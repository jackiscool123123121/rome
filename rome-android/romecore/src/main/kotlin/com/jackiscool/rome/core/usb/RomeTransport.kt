package com.jackiscool.rome.core.usb

/**
 * Raw byte-level transport to the SP-1's USB CDC-ACM data interface.
 *
 * Mirrors the shape rome-core's planned `Transport` trait will take on the Rust
 * side (see AGENTS.md handoff note in jack-rome once that refactor lands): desktop
 * implements it over `rusb`, Android implements it here over
 * `UsbDeviceConnection.bulkTransfer`. Everything above this layer (command framing,
 * battery/upload protocol, IMA-ADPCM encoding) stays in Rust and is reused as-is —
 * this interface is intentionally as thin as the existing rusb-backed desktop path.
 */
interface RomeTransport {
    /** Write raw bytes out to the device. Returns false on failure. */
    fun write(data: ByteArray): Boolean

    /**
     * Read up to [maxLen] bytes, waiting up to [timeoutMs]. Returns an empty
     * array on timeout — callers (the rome-core protocol state machine) already
     * handle short/zero reads as "keep polling" or "timed out", matching the
     * desktop rusb behavior.
     */
    fun read(maxLen: Int, timeoutMs: Int): ByteArray

    fun close()
}
