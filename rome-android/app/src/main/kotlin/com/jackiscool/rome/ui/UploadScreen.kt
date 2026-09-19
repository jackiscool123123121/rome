package com.jackiscool.rome.ui

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.jackiscool.rome.core.RomeDevice
import com.jackiscool.rome.core.SongSource
import com.jackiscool.rome.core.UploadProgress
import kotlinx.coroutines.launch

/**
 * Combined-WAV or 4-stem upload, mirroring the desktop rome-gui's own dropdown
 * choice (see jack-rome/rome-gui's queue tab). File picking uses the Storage
 * Access Framework so no storage permission is needed.
 */
@Composable
fun UploadScreen(device: RomeDevice, isDemo: Boolean, modifier: Modifier = Modifier) {
    var songName by remember { mutableStateOf("") }
    var combinedUri by remember { mutableStateOf<Uri?>(null) }
    var stemUris by remember { mutableStateOf<List<Uri>>(emptyList()) }
    var progress by remember { mutableStateOf<UploadProgress?>(null) }
    val scope = rememberCoroutineScope()

    val pickCombined = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri -> if (uri != null) { combinedUri = uri; stemUris = emptyList() } }

    val pickStems = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments()
    ) { uris -> if (uris.isNotEmpty()) { stemUris = uris; combinedUri = null } }

    Column(
        modifier = modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Top,
    ) {
        if (isDemo) {
            Text(
                "DEMO DEVICE — upload is simulated, nothing is actually sent",
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.padding(bottom = 16.dp),
            )
        }

        OutlinedTextField(
            value = songName,
            onValueChange = { songName = it },
            label = { Text("Song name") },
            modifier = Modifier.fillMaxWidth(),
        )

        Column(modifier = Modifier.padding(top = 16.dp)) {
            Button(onClick = { pickCombined.launch(arrayOf("audio/*")) }) {
                Text("Choose Combined WAV")
            }
            Text(
                combinedUri?.lastPathSegment ?: "No file chosen",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(top = 4.dp, bottom = 16.dp),
            )

            Button(onClick = { pickStems.launch(arrayOf("audio/*")) }) {
                Text("Choose 4 Stem Files")
            }
            Text(
                if (stemUris.isEmpty()) "No files chosen" else "${stemUris.size} file(s) chosen",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(top = 4.dp, bottom = 16.dp),
            )
        }

        val source: SongSource? = when {
            combinedUri != null -> SongSource.Combined(combinedUri!!)
            stemUris.size == 4 -> SongSource.Stems(stemUris)
            else -> null
        }

        Button(
            enabled = source != null && songName.isNotBlank() &&
                (progress == null || progress is UploadProgress.Done || progress is UploadProgress.Failed),
            onClick = {
                val s = source ?: return@Button
                scope.launch {
                    device.uploadSong(songName, s).collect { p -> progress = p }
                }
            },
        ) {
            Text("Upload")
        }

        val p = progress
        if (p != null) {
            Column(modifier = Modifier.padding(top = 24.dp)) {
                when (p) {
                    is UploadProgress.Encoding -> {
                        Text("Encoding stem ${p.stem}/${p.totalStems}...")
                        LinearProgressIndicator(
                            progress = { p.stem / p.totalStems.toFloat() },
                            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                        )
                    }
                    is UploadProgress.Writing -> {
                        val frac = if (p.blocksTotal > 0) p.blocksWritten / p.blocksTotal.toFloat() else 0f
                        Text("Writing to device: ${(frac * 100).toInt()}%")
                        LinearProgressIndicator(
                            progress = { frac },
                            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                        )
                    }
                    is UploadProgress.Done -> Text("Done!", style = MaterialTheme.typography.titleMedium)
                    is UploadProgress.Failed -> Text(
                        "Failed: ${p.message}",
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        }

        if (stemUris.isNotEmpty() && stemUris.size != 4) {
            Text(
                "Stem upload needs exactly 4 files.",
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
    }
}
