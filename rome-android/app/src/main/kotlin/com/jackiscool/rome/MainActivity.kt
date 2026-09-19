package com.jackiscool.rome

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // No special handling needed for the USB_DEVICE_ATTACHED launch intent here:
        // RomeUsbManager.findSp1()/connect() (invoked by ConnectionViewModel.init)
        // enumerates the currently attached UsbDeviceManager.deviceList regardless of
        // which intent started the activity, so a fresh cold-launch from the OS's
        // "open with Rome?" USB-attach prompt is already handled by the normal
        // connect() flow.
        setContent { RomeApp() }
    }
}
