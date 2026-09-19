package com.jackiscool.rome.core.usb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log
import androidx.core.content.ContextCompat
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.callbackFlow

private const val TAG = "RomeUsbManager"
private const val ACTION_USB_PERMISSION = "com.jackiscool.rome.USB_PERMISSION"

sealed interface UsbConnectState {
    data object NoDevice : UsbConnectState
    data object PermissionRequested : UsbConnectState
    data object PermissionDenied : UsbConnectState
    data class Connected(val transport: RomeTransport) : UsbConnectState
    data class Error(val message: String) : UsbConnectState
}

/**
 * Finds the SP-1 among attached USB devices, requests permission if needed, and
 * hands back a ready [RomeTransport] once the user grants it. One instance per
 * MainActivity lifecycle (see ConnectionViewModel).
 */
class RomeUsbManager(private val context: Context) {

    private val usbManager = context.getSystemService(Context.USB_SERVICE) as UsbManager

    fun findSp1(): UsbDevice? =
        usbManager.deviceList.values.firstOrNull {
            it.vendorId == SP1_VENDOR_ID && it.productId == SP1_PRODUCT_ID
        }

    /**
     * Emits connection-state updates as a cold flow: NoDevice if the SP-1 isn't
     * attached, then PermissionRequested while the system dialog is up, then
     * either Connected (with a live transport) or PermissionDenied/Error.
     */
    fun connect(): kotlinx.coroutines.flow.Flow<UsbConnectState> = callbackFlow {
        val device = findSp1()
        if (device == null) {
            trySend(UsbConnectState.NoDevice)
            close()
            return@callbackFlow
        }

        if (usbManager.hasPermission(device)) {
            send(openDevice(device))
            close()
            return@callbackFlow
        }

        val receiver = object : BroadcastReceiver() {
            override fun onReceive(ctx: Context, intent: Intent) {
                if (intent.action != ACTION_USB_PERMISSION) return
                val granted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)
                trySend(
                    if (granted) openDevice(device) else UsbConnectState.PermissionDenied
                )
                close()
            }
        }
        val filter = IntentFilter(ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            ContextCompat.registerReceiver(context, receiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
        }

        val permissionIntent = android.app.PendingIntent.getBroadcast(
            context, 0, Intent(ACTION_USB_PERMISSION),
            android.app.PendingIntent.FLAG_MUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT,
        )
        trySend(UsbConnectState.PermissionRequested)
        usbManager.requestPermission(device, permissionIntent)

        awaitClose { context.unregisterReceiver(receiver) }
    }

    private fun openDevice(device: UsbDevice): UsbConnectState {
        val connection = usbManager.openDevice(device)
            ?: return UsbConnectState.Error("openDevice failed (device may have been unplugged)")
        val transport = UsbSerialTransport.open(device, connection)
            ?: run {
                connection.close()
                return UsbConnectState.Error("no CDC-data interface with bulk IN/OUT found")
            }
        Log.i(TAG, "connected to SP-1 (vendor=${device.vendorId} product=${device.productId})")
        return UsbConnectState.Connected(transport)
    }
}
