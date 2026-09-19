package com.jackiscool.rome.core

import com.jackiscool.rome.core.usb.RomeTransport
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * marisko's USB command protocol, empirically confirmed against a real SP-1
 * attached over USB on 2026-09-19 (see /scratchpad/probe_sp1.py in this
 * project's history -- not carried into the app, kept as a one-off verification
 * script). Two stable, repeated reads of the BATTERY command both returned a
 * clean, internally-consistent 8-byte reply, confirming this exact framing:
 *
 *   Host -> Device:   [cmd:1][payload_len:4 LE][payload:N]
 *   Device -> Host:   [status:1][payload_len:4 LE][payload:N]   (status 0 = OK)
 *
 * This mirrors usb.c's own send_ok()/send_err() on the firmware side (confirmed
 * in an earlier part of this project's history, not reconstructed from this
 * probe alone) -- symmetric framing in both directions, as expected.
 *
 * The song-upload (write) opcodes below were NOT guessed or probed blind --
 * they were extracted by disassembling the real, already-working `rome` CLI
 * binary's DeviceConn::song_begin/song_multiblock/song_commit methods (ARM64
 * Mach-O, unstripped Rust symbols) on 2026-09-19, while /Volumes/LLMDATA
 * (holding the actual rome-core source) was unplugged. Full method and the
 * structural evidence for each fact are in romecore/BRIDGE_TODO.md. Summary:
 *   - SONG_BEGIN=0x04: payload = 24-byte zero-padded song name (max 23 bytes
 *     + implicit nul) + audio_block_count:u32 LE + level_block_count:u32 LE.
 *     Response payload is >=2 bytes (an assigned value the device returns);
 *     not needed for a successful upload, only validated for length.
 *   - SONG_MULTIBLOCK=0x09: sent as a RAW buffer via bulk transfer, bypassing
 *     the normal request builder -- header declares only a 2-byte payload
 *     (the block count), and the actual block data trails after it
 *     undeclared: [0x09][0x02,0,0,0][num_blocks:u16 LE][num_blocks*512 bytes
 *     of raw ADPCM block data]. Response uses the normal envelope.
 *   - SONG_COMMIT=0x06: no payload, finalizes the catalog entry.
 * Uploading with level_block_count=0 (no baked VU levels) is a real,
 * documented-elsewhere supported mode ("v1 discs" fall back to on-device
 * peak metering), not a guess -- deliberately used here to avoid needing to
 * also reverse-engineer bake_stem_levels.
 */
object RomeProtocol {
    const val CMD_BATTERY = 0x12
    const val CMD_SONG_BEGIN = 0x04
    const val CMD_SONG_MULTIBLOCK = 0x09
    const val CMD_SONG_COMMIT = 0x06
    const val STATUS_OK: Byte = 0x00
    const val NAME_FIELD_LEN = 24
    const val MAX_NAME_BYTES = 23
    const val BLOCKS_PER_MULTIBLOCK_CHUNK = 96 // matches rome-core's own chunk size (0x60)

    class ProtocolException(message: String) : IOException(message)

    private fun buildRequest(cmd: Int, payload: ByteArray): ByteArray {
        val buf = ByteBuffer.allocate(5 + payload.size).order(ByteOrder.LITTLE_ENDIAN)
        buf.put(cmd.toByte())
        buf.putInt(payload.size)
        buf.put(payload)
        return buf.array()
    }

    /** Sends [cmd] with [payload] and returns the response payload, or throws. */
    fun request(
        transport: RomeTransport,
        cmd: Int,
        payload: ByteArray = ByteArray(0),
        readTimeoutMs: Int = 2000,
    ): ByteArray {
        if (!transport.write(buildRequest(cmd, payload))) {
            throw ProtocolException("write failed for cmd 0x${cmd.toString(16)}")
        }

        // Header is a fixed 5 bytes; the device may (per the desktop client's own
        // handling of this framing) deliver it in more than one USB packet, so
        // accumulate until we have at least 5 bytes before parsing the length.
        val header = readExactly(transport, 5, readTimeoutMs)
            ?: throw ProtocolException("timed out waiting for response header (cmd 0x${cmd.toString(16)})")

        val status = header[0]
        val len = ByteBuffer.wrap(header, 1, 4).order(ByteOrder.LITTLE_ENDIAN).int
        if (status != STATUS_OK) {
            throw ProtocolException("device returned error status for cmd 0x${cmd.toString(16)}")
        }
        if (len == 0) return ByteArray(0)
        return readExactly(transport, len, readTimeoutMs)
            ?: throw ProtocolException("timed out reading $len-byte payload (cmd 0x${cmd.toString(16)})")
    }

    /** Builds a SONG_BEGIN payload: 24-byte zero-padded name + two u32 LE counts. */
    fun buildSongBeginPayload(name: String, audioBlocks: Int, levelBlocks: Int): ByteArray {
        val nameBytes = name.encodeToByteArray()
        require(nameBytes.size <= MAX_NAME_BYTES) { "song name too long (${nameBytes.size} > $MAX_NAME_BYTES bytes)" }
        val buf = ByteBuffer.allocate(NAME_FIELD_LEN + 8).order(ByteOrder.LITTLE_ENDIAN)
        buf.put(nameBytes)
        buf.position(NAME_FIELD_LEN) // rest of the 24-byte field stays zero (padding)
        buf.putInt(audioBlocks)
        buf.putInt(levelBlocks)
        return buf.array()
    }

    /**
     * Builds the raw SONG_MULTIBLOCK wire buffer: 5-byte header (declaring only
     * the 2-byte block-count payload) + the block-count itself + the raw block
     * data appended directly after, undeclared by the header length. See the
     * class doc comment -- this is the one command that doesn't go through
     * [buildRequest]'s normal framing.
     */
    fun buildMultiblockRequest(blockData: ByteArray, blockOffset: Int, numBlocks: Int): ByteArray {
        val dataLen = numBlocks * 512
        val buf = ByteBuffer.allocate(5 + 2 + dataLen).order(ByteOrder.LITTLE_ENDIAN)
        buf.put(CMD_SONG_MULTIBLOCK.toByte())
        buf.putInt(2) // declared payload length covers only the u16 block count
        buf.putShort(numBlocks.toShort())
        buf.put(blockData, blockOffset, dataLen)
        return buf.array()
    }

    /** Sends a pre-built raw buffer (see [buildMultiblockRequest]) and reads the normal response envelope. */
    fun requestRaw(transport: RomeTransport, rawBuffer: ByteArray, readTimeoutMs: Int = 5000): ByteArray {
        if (!transport.write(rawBuffer)) throw ProtocolException("write failed (multiblock)")
        val header = readExactly(transport, 5, readTimeoutMs)
            ?: throw ProtocolException("timed out waiting for response header (multiblock)")
        val status = header[0]
        val len = ByteBuffer.wrap(header, 1, 4).order(ByteOrder.LITTLE_ENDIAN).int
        if (status != STATUS_OK) throw ProtocolException("device returned error status (multiblock)")
        if (len == 0) return ByteArray(0)
        return readExactly(transport, len, readTimeoutMs)
            ?: throw ProtocolException("timed out reading $len-byte payload (multiblock)")
    }

    private fun readExactly(transport: RomeTransport, n: Int, timeoutMs: Int): ByteArray? {
        val out = ByteArray(n)
        var got = 0
        val deadline = System.currentTimeMillis() + timeoutMs
        while (got < n) {
            val remaining = deadline - System.currentTimeMillis()
            if (remaining <= 0) return null
            val chunk = transport.read(n - got, remaining.toInt().coerceAtLeast(1))
            if (chunk.isEmpty()) continue
            chunk.copyInto(out, got)
            got += chunk.size
        }
        return out
    }
}
