package com.jackiscool.rome.core

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.flow
import kotlin.random.Random

/**
 * Simulated SP-1 for emulator testing. The Android emulator has no real USB-host
 * passthrough for arbitrary attached devices (this was verified, not assumed — see
 * BRIDGE_TODO.md), so end-to-end hardware validation needs a physical Android
 * device over USB-OTG. This lets the UI, navigation, and upload-progress flow be
 * exercised and screenshotted on the emulator in the meantime. Never used unless
 * the user explicitly picks "Use Demo Device" — real hardware is always preferred
 * when present.
 */
class FakeRomeDevice : RomeDevice {
    private var chargeTick = 0

    override suspend fun battery(): BatteryStatus {
        delay(300) // feels like a real USB round trip
        chargeTick++
        return BatteryStatus(
            percent = (60 + (chargeTick * 3) % 40),
            charging = chargeTick % 4 == 0,
            usbPresent = true,
        )
    }

    override fun uploadSong(name: String, source: SongSource) = flow {
        val stemCount = when (source) {
            is SongSource.Combined -> 1
            is SongSource.Stems -> source.uris.size
        }
        for (s in 1..stemCount) {
            emit(UploadProgress.Encoding(s, stemCount))
            delay(400)
        }
        val totalBlocks = 4000L + Random.nextLong(0, 2000)
        var written = 0L
        while (written < totalBlocks) {
            written = (written + 250 + Random.nextLong(0, 150)).coerceAtMost(totalBlocks)
            emit(UploadProgress.Writing(written, totalBlocks))
            delay(120)
        }
        emit(UploadProgress.Done)
    }

    override fun close() {}
}
