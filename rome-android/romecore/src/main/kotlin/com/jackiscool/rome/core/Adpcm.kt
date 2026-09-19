package com.jackiscool.rome.core

/**
 * IMA-ADPCM encoder matching marisko's on-device format, reverse-engineered
 * from the real compiled `rome` CLI binary's `rome_core::adpcm::encode_block_8ch`
 * (disassembled 2026-09-19 while /Volumes/LLMDATA -- holding the actual source --
 * was unplugged; see romecore/BRIDGE_TODO.md for the full method and why this
 * approach was used instead of guessing).
 *
 * VERIFIED structural facts from disassembly (not assumed):
 *  - Standard IMA-ADPCM: the step-size/index tables and the diff/sign/clamp
 *    algorithm exactly match the public IMA Digital Audio Focus Group spec
 *    (unchanged industry standard since 1992). Confirmed by exact constants
 *    seen in the disassembly: predictor clamped to [-0x8000, 0x7fff]
 *    (-32768..32767), step index clamped to max 0x58 (88) -- both match the
 *    standard 89-entry step table's last index.
 *  - NO per-block header (no embedded predictor/step-index preamble like
 *    standard WAV-ADPCM files use) -- predictor/index state is continuous
 *    across the whole song per channel, carried externally.
 *  - 128 samples per channel per 512-byte block: encode_8ch computes
 *    block_count = ceil(sample_count / 128), and encode_block_8ch's inner
 *    loop runs 128 times per channel, writing 2 samples/byte (nibble-packed)
 *    = 64 bytes/channel.
 *  - 8 channels occupy contiguous 64-byte regions within each 512-byte block,
 *    in channel order 0..7 (not sample-interleaved across channels) --
 *    encode_block_8ch's outer loop advances the output cursor by 0x40 (64)
 *    per channel, 8 iterations = 512 bytes total.
 */
object Adpcm {
    private val STEP_TABLE = intArrayOf(
        7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31,
        34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143,
        157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658,
        724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024,
        3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899,
        15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
    )
    private val INDEX_TABLE = intArrayOf(-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8)

    private const val SAMPLES_PER_BLOCK = 128
    const val BYTES_PER_CHANNEL_PER_BLOCK = 64
    const val CHANNELS_PER_BLOCK = 8
    const val BLOCK_SIZE = BYTES_PER_CHANNEL_PER_BLOCK * CHANNELS_PER_BLOCK // 512

    private class ChannelState {
        var predictor = 0
        var index = 0
    }

    /**
     * Encodes one channel's full sample stream into consecutive 64-byte
     * chunks (one per block), zero-padding the final partial block with
     * silence (sample = 0) the same way encode_song zero-pads shorter stems
     * to the longest one before calling into the 8ch encoder.
     */
    private fun encodeChannel(samples: ShortArray, blockCount: Int): ByteArray {
        val out = ByteArray(blockCount * BYTES_PER_CHANNEL_PER_BLOCK)
        val st = ChannelState()
        var outPos = 0
        var samplePos = 0
        val total = blockCount * SAMPLES_PER_BLOCK
        var pendingNibble = -1 // -1 = no pending high nibble
        while (samplePos < total) {
            val sample = if (samplePos < samples.size) samples[samplePos].toInt() else 0
            val code = encodeSample(sample, st)
            if (pendingNibble < 0) {
                pendingNibble = code
            } else {
                out[outPos++] = ((pendingNibble and 0x0F) or ((code and 0x0F) shl 4)).toByte()
                pendingNibble = -1
            }
            samplePos++
        }
        return out
    }

    private fun encodeSample(sampleIn: Int, st: ChannelState): Int {
        var diff = sampleIn - st.predictor
        var sign = 0
        if (diff < 0) {
            sign = 8
            diff = -diff
        }
        val step = STEP_TABLE[st.index]
        var code = 0
        var diffq = step shr 3
        var tempStep = step
        if (diff >= tempStep) {
            code = code or 4
            diff -= tempStep
            diffq += tempStep
        }
        tempStep = tempStep shr 1
        if (diff >= tempStep) {
            code = code or 2
            diff -= tempStep
            diffq += tempStep
        }
        tempStep = tempStep shr 1
        if (diff >= tempStep) {
            code = code or 1
            diffq += tempStep
        }
        code = code or sign

        st.predictor = if (sign != 0) st.predictor - diffq else st.predictor + diffq
        st.predictor = st.predictor.coerceIn(-32768, 32767)

        st.index += INDEX_TABLE[code]
        st.index = st.index.coerceIn(0, STEP_TABLE.size - 1)

        return code
    }

    /**
     * Encodes 8 mono channels (already zero-padded to equal length by the
     * caller, matching encode_song's own padding behavior) into a single
     * concatenated byte buffer of `blockCount` 512-byte blocks, each block
     * being 8 consecutive 64-byte per-channel chunks in channel order.
     */
    fun encode8ch(channels: List<ShortArray>, blockCount: Int): ByteArray {
        require(channels.size == CHANNELS_PER_BLOCK) { "expected 8 channels, got ${channels.size}" }
        val perChannel = channels.map { encodeChannel(it, blockCount) }
        val out = ByteArray(blockCount * BLOCK_SIZE)
        for (block in 0 until blockCount) {
            val blockOff = block * BLOCK_SIZE
            for (ch in 0 until CHANNELS_PER_BLOCK) {
                val chOff = block * BYTES_PER_CHANNEL_PER_BLOCK
                System.arraycopy(
                    perChannel[ch], chOff,
                    out, blockOff + ch * BYTES_PER_CHANNEL_PER_BLOCK,
                    BYTES_PER_CHANNEL_PER_BLOCK,
                )
            }
        }
        return out
    }
}
