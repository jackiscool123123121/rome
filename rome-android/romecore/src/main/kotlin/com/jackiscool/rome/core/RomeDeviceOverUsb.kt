package com.jackiscool.rome.core

import android.content.ContentResolver
import android.net.Uri
import com.jackiscool.rome.core.usb.RomeTransport
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.withContext

/**
 * Real [RomeDevice] talking to a physical SP-1 over [transport]. Both battery
 * (read) and song upload (write) are implemented against the real, verified
 * protocol -- battery confirmed live against physical hardware, upload
 * extracted from the real compiled `rome` CLI binary via disassembly (see
 * RomeProtocol.kt's doc comment and romecore/BRIDGE_TODO.md for the full
 * method and evidence -- this was NOT guessed or probed blind).
 */
class RomeDeviceOverUsb(
    private val transport: RomeTransport,
    private val contentResolver: ContentResolver,
) : RomeDevice {

    override suspend fun battery(): BatteryStatus = withContext(Dispatchers.IO) {
        val payload = RomeProtocol.request(transport, RomeProtocol.CMD_BATTERY)
        require(payload.size >= 3) { "battery response too short: ${payload.size} bytes" }
        val rawPercent = payload[0].toInt() and 0xFF
        BatteryStatus(
            percent = if (rawPercent == 0xFF) null else rawPercent,
            charging = payload[1].toInt() != 0,
            usbPresent = payload[2].toInt() != 0,
        )
    }

    override fun uploadSong(name: String, source: SongSource) = flow {
        // 1. Load stems as 4 stereo pairs (8 mono channels), channel order 0-3.
        val stemUris: List<Uri> = when (source) {
            is SongSource.Combined -> List(4) { source.uri } // see SongSource's doc comment
            is SongSource.Stems -> {
                require(source.uris.size == 4) { "expected 4 stem files, got ${source.uris.size}" }
                source.uris
            }
        }

        emit(UploadProgress.Encoding(0, 4))
        val stems = stemUris.mapIndexed { i, uri ->
            emit(UploadProgress.Encoding(i + 1, 4))
            openStereoPcm(uri)
        }

        // 2. Zero-pad every stem's channels to the longest one (matches
        // encode_song's own padding behavior, confirmed via disassembly).
        val maxSamples = stems.maxOf { maxOf(it.left.size, it.right.size) }
        val channels: List<ShortArray> = stems.flatMap { listOf(it.left, it.right) }
            .map { it.copyOf(maxSamples) } // ShortArray.copyOf zero-pads, matching silence-padding

        val blockCount = (maxSamples + 127) / 128 // ceil(samples / 128), verified via disassembly
        if (blockCount <= 0) {
            emit(UploadProgress.Failed("song is empty (no audio samples)"))
            return@flow
        }

        val encoded = Adpcm.encode8ch(channels, blockCount)

        // 3. song_begin: allocate the catalog entry. level_blocks=0 -- no
        // baked VU levels, a real supported mode (see RomeProtocol's doc).
        val beginPayload = RomeProtocol.buildSongBeginPayload(name, blockCount, 0)
        val beginResp = withContext(Dispatchers.IO) {
            RomeProtocol.request(transport, RomeProtocol.CMD_SONG_BEGIN, beginPayload, readTimeoutMs = 5000)
        }
        if (beginResp.size < 2) {
            emit(UploadProgress.Failed("song_begin: response too short (${beginResp.size} bytes)"))
            return@flow
        }

        // 4. song_multiblock in chunks (matches rome-core's own 96-block chunking).
        var blocksWritten = 0
        try {
            while (blocksWritten < blockCount) {
                val chunkBlocks = minOf(RomeProtocol.BLOCKS_PER_MULTIBLOCK_CHUNK, blockCount - blocksWritten)
                val raw = RomeProtocol.buildMultiblockRequest(
                    encoded, blocksWritten * Adpcm.BLOCK_SIZE, chunkBlocks,
                )
                withContext(Dispatchers.IO) {
                    RomeProtocol.requestRaw(transport, raw, readTimeoutMs = 10000)
                }
                blocksWritten += chunkBlocks
                emit(UploadProgress.Writing(blocksWritten.toLong(), blockCount.toLong()))
            }
        } catch (e: Exception) {
            emit(UploadProgress.Failed("upload failed after $blocksWritten of $blockCount blocks: ${e.message}"))
            return@flow
        }

        // 5. song_commit: finalize the catalog entry.
        withContext(Dispatchers.IO) {
            RomeProtocol.request(transport, RomeProtocol.CMD_SONG_COMMIT, readTimeoutMs = 5000)
        }
        emit(UploadProgress.Done)
    }

    private suspend fun openStereoPcm(uri: Uri): StereoPcm16 = withContext(Dispatchers.IO) {
        contentResolver.openInputStream(uri)?.use { WavFile.readStereo16(it) }
            ?: throw RomeProtocol.ProtocolException("could not open $uri")
    }

    override fun close() = transport.close()
}
