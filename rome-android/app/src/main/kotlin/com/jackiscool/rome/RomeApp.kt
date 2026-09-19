package com.jackiscool.rome

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BatteryFull
import androidx.compose.material.icons.filled.CloudUpload
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.jackiscool.rome.ui.BatteryScreen
import com.jackiscool.rome.ui.ConnectScreen
import com.jackiscool.rome.ui.UploadScreen

private object Routes {
    const val BATTERY = "battery"
    const val UPLOAD = "upload"
}

@Composable
fun RomeApp(viewModel: ConnectionViewModel = viewModel()) {
    val state by viewModel.state.collectAsState()

    MaterialTheme {
        when (val s = state) {
            is AppConnectionState.Ready -> ReadyScaffold(s)
            else -> ConnectScreen(
                state = s,
                onRetry = viewModel::connect,
                onUseDemoDevice = viewModel::useDemoDevice,
            )
        }
    }
}

@Composable
private fun ReadyScaffold(ready: AppConnectionState.Ready) {
    val navController = rememberNavController()

    Scaffold(
        bottomBar = {
            NavigationBar {
                val backStackEntry by navController.currentBackStackEntryAsState()
                val currentDestination = backStackEntry?.destination

                NavigationBarItem(
                    selected = currentDestination?.hierarchy?.any { it.route == Routes.BATTERY } == true,
                    onClick = {
                        navController.navigate(Routes.BATTERY) {
                            popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                            launchSingleTop = true
                            restoreState = true
                        }
                    },
                    icon = { Icon(Icons.Filled.BatteryFull, contentDescription = "Battery") },
                    label = { Text("Battery") },
                )
                NavigationBarItem(
                    selected = currentDestination?.hierarchy?.any { it.route == Routes.UPLOAD } == true,
                    onClick = {
                        navController.navigate(Routes.UPLOAD) {
                            popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                            launchSingleTop = true
                            restoreState = true
                        }
                    },
                    icon = { Icon(Icons.Filled.CloudUpload, contentDescription = "Upload") },
                    label = { Text("Upload") },
                )
            }
        },
    ) { padding ->
        NavHost(
            navController = navController,
            startDestination = Routes.BATTERY,
            modifier = Modifier.padding(padding),
        ) {
            composable(Routes.BATTERY) { BatteryScreen(ready.device, ready.isDemo) }
            composable(Routes.UPLOAD) { UploadScreen(ready.device, ready.isDemo) }
        }
    }
}
