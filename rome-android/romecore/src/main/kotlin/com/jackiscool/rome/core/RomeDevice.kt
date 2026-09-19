package com.jackiscool.rome.core

import android.net.Uri

/** Mirrors marisko's USB_CMD_BATTERY reply (percent/charging/usb_present), see rome-core proto.rs. */
data class BatteryStatus(
    val percent: Int?,      // null if the device's ADC read failed (0xFF sentinel)
    val charging: Boolean,
    val usbPresent: Boolean,
)

sealed interface UploadProgress {
    data class Encoding(val stem: Int, val totalStems: Int) : UploadProgress
    data class Writing(val blocksWritten: Long, val blocksTotal: Long) : UploadProgress
    data object Done : UploadProgress
    data class Failed(val message: String) : UploadProgress
}

/**
 * One combined stereo WAV (duplicated across all 4 stem slots -- see
 * BRIDGE_TODO.md for why: rome-core's own encode_song loads a fixed list of
 * stem files, one call to song_begin's 8-channel encoder needs 4 stereo
 * sources, and duplicating a single mix into all 4 is the safe, degrade-
 * gracefully choice when the exact desktop CLI behavior for this specific
 * mode couldn't be confirmed from disassembly alone), or four stereo stem
 * WAVs in channel order 0-3 (matching this codebase's stem-index convention
 * used throughout marisko's gate effect / fader gains).
 */
sealed interface SongSource {
    data class Combined(val uri: Uri) : SongSource
    data class Stems(val uris: List<Uri>) : SongSource
}

/**
 * High-level device API the UI talks to. Backed by either [FakeRomeDevice] (emulator
 * demo, no hardware) or the real rome-core bridge over USB (see RustRomeDevice.kt /
 * BRIDGE_TODO.md for what's still needed to wire that up).
 */
interface RomeDevice {
    suspend fun battery(): BatteryStatus

    /** Emits progress updates as the song is encoded and streamed to the device. */
    fun uploadSong(name: String, source: SongSource): kotlinx.coroutines.flow.Flow<UploadProgress>

    fun close()
}
