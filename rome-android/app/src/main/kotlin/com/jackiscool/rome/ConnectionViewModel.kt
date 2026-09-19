package com.jackiscool.rome

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.jackiscool.rome.core.FakeRomeDevice
import com.jackiscool.rome.core.RomeDevice
import com.jackiscool.rome.core.RomeDeviceOverUsb
import com.jackiscool.rome.core.usb.RomeUsbManager
import com.jackiscool.rome.core.usb.UsbConnectState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

sealed interface AppConnectionState {
    data object Searching : AppConnectionState
    data object NoDevice : AppConnectionState
    data object AwaitingPermission : AppConnectionState
    data object PermissionDenied : AppConnectionState
    data class Error(val message: String) : AppConnectionState
    data class Ready(val device: RomeDevice, val isDemo: Boolean) : AppConnectionState
}

class ConnectionViewModel(application: Application) : AndroidViewModel(application) {
    private val usbManager = RomeUsbManager(application)

    private val _state = MutableStateFlow<AppConnectionState>(AppConnectionState.Searching)
    val state: StateFlow<AppConnectionState> = _state.asStateFlow()

    init {
        connect()
    }

    fun connect() {
        viewModelScope.launch {
            usbManager.connect().collect { s ->
                _state.value = when (s) {
                    is UsbConnectState.NoDevice -> AppConnectionState.NoDevice
                    is UsbConnectState.PermissionRequested -> AppConnectionState.AwaitingPermission
                    is UsbConnectState.PermissionDenied -> AppConnectionState.PermissionDenied
                    is UsbConnectState.Error -> AppConnectionState.Error(s.message)
                    is UsbConnectState.Connected ->
                        AppConnectionState.Ready(
                            RomeDeviceOverUsb(s.transport, getApplication<Application>().contentResolver),
                            isDemo = false,
                        )
                }
            }
        }
    }

    fun useDemoDevice() {
        _state.value = AppConnectionState.Ready(FakeRomeDevice(), isDemo = true)
    }

    override fun onCleared() {
        (_state.value as? AppConnectionState.Ready)?.device?.close()
    }
}
