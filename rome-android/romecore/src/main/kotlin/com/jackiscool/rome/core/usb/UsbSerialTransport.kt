package com.jackiscool.rome.core.usb

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.util.Log

private const val TAG = "UsbSerialTransport"

/** VID/PID for the SP-1's USB CDC-ACM interface (see usb_device_filter.xml). */
const val SP1_VENDOR_ID = 0x2FE3
const val SP1_PRODUCT_ID = 0x0101

/**
 * [RomeTransport] backed by a claimed USB interface's bulk IN/OUT endpoints.
 *
 * A CDC-ACM device exposes (at least) two interfaces: a Communications
 * Class interface (control) and a CDC-Data interface (the actual bulk
 * IN/OUT pipe rome-core's protocol runs over — this mirrors exactly what
 * `rusb` claims on desktop; see jack-rome/rome-core/src/proto.rs's own
 * comment about claiming the CDC-data interface). We scan every interface
 * on the device for one with a bulk IN and bulk OUT endpoint rather than
 * hardcoding an interface index, since that index isn't guaranteed stable
 * across USB stack/driver versions.
 */
class UsbSerialTransport private constructor(
    private val connection: UsbDeviceConnection,
    private val iface: UsbInterface,
    private val epIn: UsbEndpoint,
    private val epOut: UsbEndpoint,
) : RomeTransport {

    override fun write(data: ByteArray): Boolean {
        if (data.isEmpty()) return true
        val n = connection.bulkTransfer(epOut, data, data.size, WRITE_TIMEOUT_MS)
        if (n != data.size) {
            Log.w(TAG, "short write: wrote $n of ${data.size} bytes")
            return false
        }
        return true
    }

    override fun read(maxLen: Int, timeoutMs: Int): ByteArray {
        val buf = ByteArray(maxLen)
        val n = connection.bulkTransfer(epIn, buf, maxLen, timeoutMs)
        if (n <= 0) return ByteArray(0)
        return buf.copyOf(n)
    }

    override fun close() {
        connection.releaseInterface(iface)
        connection.close()
    }

    companion object {
        private const val WRITE_TIMEOUT_MS = 2000

        /**
         * Claim the CDC-data interface and return a ready-to-use transport, or
         * null if [device]/[connection] don't expose the bulk endpoints we need
         * (wrong device, or the interface claim was rejected).
         */
        fun open(device: UsbDevice, connection: UsbDeviceConnection): UsbSerialTransport? {
            for (i in 0 until device.interfaceCount) {
                val iface = device.getInterface(i)
                val epIn = iface.findBulkEndpoint(UsbConstants.USB_DIR_IN) ?: continue
                val epOut = iface.findBulkEndpoint(UsbConstants.USB_DIR_OUT) ?: continue
                if (!connection.claimInterface(iface, true)) {
                    Log.w(TAG, "claimInterface failed for interface $i")
                    continue
                }
                Log.i(TAG, "claimed interface $i (bulk in=${epIn.address} out=${epOut.address})")
                return UsbSerialTransport(connection, iface, epIn, epOut)
            }
            Log.e(TAG, "no interface with both bulk IN and bulk OUT endpoints found")
            return null
        }

        private fun UsbInterface.findBulkEndpoint(direction: Int): UsbEndpoint? {
            for (i in 0 until endpointCount) {
                val ep = getEndpoint(i)
                if (ep.type == UsbConstants.USB_ENDPOINT_XFER_BULK && ep.direction == direction) {
                    return ep
                }
            }
            return null
        }
    }
}
