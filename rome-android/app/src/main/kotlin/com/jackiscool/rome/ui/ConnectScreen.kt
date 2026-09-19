package com.jackiscool.rome.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.jackiscool.rome.AppConnectionState

@Composable
fun ConnectScreen(
    state: AppConnectionState,
    onRetry: () -> Unit,
    onUseDemoDevice: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        when (state) {
            is AppConnectionState.Searching -> {
                CircularProgressIndicator()
                Text("Looking for SP-1...", modifier = Modifier.padding(top = 16.dp))
            }
            is AppConnectionState.AwaitingPermission -> {
                CircularProgressIndicator()
                Text("Waiting for USB permission...", modifier = Modifier.padding(top = 16.dp))
            }
            is AppConnectionState.NoDevice -> {
                Text("No SP-1 found", style = MaterialTheme.typography.titleLarge)
                Text(
                    "Plug in the stem player over USB-C, or try the demo mode below.",
                    modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Button(onClick = onRetry) { Text("Retry") }
            }
            is AppConnectionState.PermissionDenied -> {
                Text("USB permission denied", style = MaterialTheme.typography.titleLarge)
                Text(
                    "Rome needs permission to talk to the SP-1 over USB.",
                    modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
                )
                Button(onClick = onRetry) { Text("Try Again") }
            }
            is AppConnectionState.Error -> {
                Text("Connection error", style = MaterialTheme.typography.titleLarge)
                Text(state.message, modifier = Modifier.padding(top = 8.dp, bottom = 24.dp))
                Button(onClick = onRetry) { Text("Retry") }
            }
            is AppConnectionState.Ready -> {
                // Handled by the caller (navigates away once Ready).
            }
        }

        if (state !is AppConnectionState.Ready && state !is AppConnectionState.Searching) {
            OutlinedButton(onClick = onUseDemoDevice, modifier = Modifier.padding(top = 12.dp)) {
                Text("Use Demo Device (no real SP-1)")
            }
        }
    }
}
