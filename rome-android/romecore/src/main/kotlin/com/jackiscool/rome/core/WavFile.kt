package com.jackiscool.rome.core

import java.io.IOException
import java.io.InputStream

/** Decoded 16-bit PCM stereo WAV audio: separate left/right sample arrays. */
data class StereoPcm16(val left: ShortArray, val right: ShortArray, val sampleRate: Int)

/**
 * Minimal RIFF/WAVE parser for 16-bit PCM (mono or stereo). Good enough for
 * song uploads -- doesn't need to handle every WAV variant, just what a user
 * would export stems as. Mono input is duplicated to both channels.
 */
object WavFile {
    class WavParseException(message: String) : IOException(message)

    fun readStereo16(input: InputStream): StereoPcm16 {
        val data = input.readBytes()
        if (data.size < 44 || !matches(data, 0, "RIFF") || !matches(data, 8, "WAVE")) {
            throw WavParseException("not a RIFF/WAVE file")
        }

        var pos = 12
        var channels = 0
        var sampleRate = 0
        var bitsPerSample = 0
        var dataOffset = -1
        var dataLen = 0

        while (pos + 8 <= data.size) {
            val chunkId = String(data, pos, 4, Charsets.US_ASCII)
            val chunkSize = readU32LE(data, pos + 4)
            val body = pos + 8
            when (chunkId) {
                "fmt " -> {
                    channels = readU16LE(data, body + 2)
                    sampleRate = readU32LE(data, body + 4).toInt()
                    bitsPerSample = readU16LE(data, body + 14)
                }
                "data" -> {
                    dataOffset = body
                    dataLen = chunkSize.toInt().coerceAtMost(data.size - body)
                }
            }
            // Chunks are word-aligned: an odd chunkSize has a padding byte.
            val advance = chunkSize.toInt() + (chunkSize.toInt() and 1)
            pos = body + advance
        }

        if (dataOffset < 0) throw WavParseException("no data chunk found")
        if (bitsPerSample != 16) throw WavParseException("only 16-bit PCM WAV is supported (got $bitsPerSample-bit)")
        if (channels != 1 && channels != 2) throw WavParseException("only mono/stereo WAV is supported (got $channels ch)")

        val bytesPerFrame = 2 * channels
        val frameCount = dataLen / bytesPerFrame
        val left = ShortArray(frameCount)
        val right = ShortArray(frameCount)
        var p = dataOffset
        for (i in 0 until frameCount) {
            val l = readS16LE(data, p)
            left[i] = l
            right[i] = if (channels == 2) readS16LE(data, p + 2) else l
            p += bytesPerFrame
        }
        return StereoPcm16(left, right, sampleRate)
    }

    private fun matches(data: ByteArray, offset: Int, s: String): Boolean {
        if (offset + s.length > data.size) return false
        for (i in s.indices) if (data[offset + i] != s[i].code.toByte()) return false
        return true
    }

    private fun readU16LE(d: ByteArray, o: Int): Int =
        (d[o].toInt() and 0xFF) or ((d[o + 1].toInt() and 0xFF) shl 8)

    private fun readU32LE(d: ByteArray, o: Int): Long =
        ((d[o].toLong() and 0xFF)) or
            ((d[o + 1].toLong() and 0xFF) shl 8) or
            ((d[o + 2].toLong() and 0xFF) shl 16) or
            ((d[o + 3].toLong() and 0xFF) shl 24)

    private fun readS16LE(d: ByteArray, o: Int): Short =
        (((d[o].toInt() and 0xFF)) or ((d[o + 1].toInt()) shl 8)).toShort()
}
