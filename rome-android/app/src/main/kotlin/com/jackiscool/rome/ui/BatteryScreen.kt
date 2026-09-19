package com.jackiscool.rome.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.jackiscool.rome.core.BatteryStatus
import com.jackiscool.rome.core.RomeDevice
import kotlinx.coroutines.delay

private const val AUTO_REFRESH_MS = 5000L

@Composable
fun BatteryScreen(device: RomeDevice, isDemo: Boolean, modifier: Modifier = Modifier) {
    var status by remember { mutableStateOf<BatteryStatus?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var refreshTick by remember { mutableStateOf(0) }

    LaunchedEffect(refreshTick) {
        error = null
        try {
            status = device.battery()
        } catch (e: Exception) {
            error = e.message ?: "unknown error"
        }
    }

    // Keep the reading live without the user having to tap refresh.
    LaunchedEffect(Unit) {
        while (true) {
            delay(AUTO_REFRESH_MS)
            refreshTick++
        }
    }

    Column(
        modifier = modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        if (isDemo) {
            Text(
                "DEMO DEVICE — simulated data, not a real SP-1",
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.padding(bottom = 24.dp),
            )
        }

        val s = status
        when {
            error != null -> {
                Text("Couldn't read battery", style = MaterialTheme.typography.titleLarge)
                Text(error ?: "", modifier = Modifier.padding(top = 8.dp, bottom = 16.dp))
                Button(onClick = { refreshTick++ }) { Text("Retry") }
            }
            s == null -> CircularProgressIndicator()
            else -> {
                Text(
                    s.percent?.let { "$it%" } ?: "—",
                    style = MaterialTheme.typography.displayLarge,
                )
                LinearProgressIndicator(
                    progress = { (s.percent ?: 0) / 100f },
                    modifier = Modifier.padding(vertical = 16.dp).fillMaxWidth(fraction = 0.7f),
                )
                Text(
                    when {
                        s.charging -> "Charging"
                        s.usbPresent -> "USB connected, not charging"
                        else -> "On battery"
                    },
                    style = MaterialTheme.typography.bodyLarge,
                )
                Button(onClick = { refreshTick++ }, modifier = Modifier.padding(top = 24.dp)) {
                    Text("Refresh Now")
                }
            }
        }
    }
}
