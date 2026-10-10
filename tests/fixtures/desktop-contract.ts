export default {
  "calls": [
    [
      "list_audio_devices"
    ],
    [
      "refresh_audio_devices"
    ],
    [
      "start_route",
      {
        "request": {
          "denoise": {
            "attenuationLimitDb": 24.0,
            "enabled": true,
            "postFilterEnabled": true
          },
          "destinationDeviceId": "fake:output",
          "mode": "speech_denoise",
          "routeId": "desktop-monitor",
          "sourceDeviceId": "fake:input",
          "targetLatencyMs": 40
        }
      }
    ],
    [
      "update_route_settings",
      {
        "request": {
          "denoise": {
            "attenuationLimitDb": 18.0,
            "enabled": true,
            "postFilterEnabled": true
          },
          "routeId": "desktop-monitor"
        }
      }
    ],
    [
      "set_route_bypass",
      {
        "request": {
          "bypassed": true,
          "routeId": "desktop-monitor"
        }
      }
    ],
    [
      "stop_route",
      {
        "routeId": "desktop-monitor"
      }
    ],
    [
      "get_engine_snapshot"
    ],
    [
      "get_settings"
    ]
  ],
  "error": {
    "code": "invalid_configuration",
    "details": null,
    "message": "Target latency must be non-zero.",
    "suggestedAction": null
  },
  "events": [
    {
      "payload": {
        "revision": 8,
        "routeId": "desktop-monitor",
        "state": "recovering",
        "type": "state_changed"
      },
      "topic": "audio://state-changed"
    },
    {
      "payload": {
        "revision": 9,
        "type": "devices_changed"
      },
      "topic": "audio://devices-changed"
    },
    {
      "payload": {
        "metrics": {
          "degraded": false,
          "discontinuities": 0,
          "driftCorrectionPpm": 0,
          "estimatedLatencyMs": 0.0,
          "inferenceLastUs": 0,
          "inferenceP95Us": 0,
          "inferenceP99Us": 0,
          "inputOverruns": 0,
          "inputPeak": 0.0,
          "inputRingFillFrames": 0,
          "inputRingHighWaterFrames": 0,
          "inputRms": 0.0,
          "missedDeadlines": 0,
          "modelResets": 0,
          "outputPeak": 0.0,
          "outputRingFillFrames": 0,
          "outputRingLowWaterFrames": 0,
          "outputRms": 0.0,
          "outputUnderruns": 0,
          "overloadEvents": 0,
          "resetLastUs": 0,
          "resetTotalUs": 0,
          "skippedInferences": 0
        },
        "revision": 10,
        "routeId": "desktop-monitor",
        "type": "metrics"
      },
      "topic": "audio://metrics"
    },
    {
      "payload": {
        "error": {
          "code": "device_disconnected",
          "details": null,
          "message": "Fixture device disappeared.",
          "suggestedAction": "Reconnect the device."
        },
        "recoverable": true,
        "revision": 11,
        "routeId": "desktop-monitor",
        "type": "fault"
      },
      "topic": "audio://fault"
    }
  ],
  "pipelineStates": [
    "stopped",
    "starting",
    "running",
    "reconfiguring",
    "recovering",
    "failed",
    "stopping"
  ],
  "settings": {
    "loadWarning": null,
    "revision": 0,
    "savedRevision": 0,
    "settings": {
      "incoming": {
        "denoise": {
          "attenuationLimitDb": null,
          "enabled": true,
          "postFilterEnabled": false
        },
        "destinationKind": "physical_output",
        "echoCancellation": false,
        "inputId": "",
        "mode": "aligned_bypass",
        "outputId": "",
        "targetLatencyMs": 40
      },
      "microphone": {
        "denoise": {
          "attenuationLimitDb": null,
          "enabled": true,
          "postFilterEnabled": false
        },
        "destinationKind": "virtual_microphone",
        "echoCancellation": true,
        "inputId": "",
        "mode": "speech_denoise",
        "outputId": "",
        "targetLatencyMs": 40
      }
    },
    "writeError": null
  },
  "snapshot": {
    "inventory": {
      "devices": [
        {
          "defaultInputFormat": {
            "bufferSizeFrames": 480,
            "channels": 1,
            "sampleFormat": "f32",
            "sampleRateHz": 48000
          },
          "defaultOutputFormat": null,
          "driver": "fixture.driver",
          "host": "fake",
          "id": "fake:input",
          "isDefaultInput": true,
          "isDefaultOutput": false,
          "kind": "physical",
          "manufacturer": "Fixture manufacturer",
          "name": "Fixture microphone",
          "supportsInput": true,
          "supportsOutput": false
        }
      ],
      "host": "fake"
    },
    "lastFault": {
      "code": "device_disconnected",
      "details": "fixture stream 2",
      "message": "Fixture headphones disappeared.",
      "suggestedAction": "Reconnect headphones."
    },
    "revision": 7,
    "routes": [
      {
        "bypassed": false,
        "config": {
          "denoise": {
            "attenuationLimitDb": null,
            "enabled": true,
            "postFilterEnabled": true
          },
          "destination": {
            "physicalOutput": "fake:output"
          },
          "echoCancellation": false,
          "id": "desktop-monitor",
          "processingMode": "speech_denoise",
          "source": {
            "physicalInput": "fake:input"
          },
          "targetLatencyMs": 40
        },
        "echoCancellation": {
          "fallbackFrames": 3,
          "maxProcessingUs": 200,
          "missedDeadlines": 0,
          "nativeErrors": 0,
          "processedFrames": 42,
          "resets": 1,
          "status": "running"
        },
        "fault": null,
        "inputDevice": {
          "defaultInputFormat": {
            "bufferSizeFrames": 480,
            "channels": 1,
            "sampleFormat": "f32",
            "sampleRateHz": 48000
          },
          "defaultOutputFormat": null,
          "driver": "fixture.driver",
          "host": "fake",
          "id": "fake:input",
          "isDefaultInput": true,
          "isDefaultOutput": false,
          "kind": "physical",
          "manufacturer": "Fixture manufacturer",
          "name": "Fixture microphone",
          "supportsInput": true,
          "supportsOutput": false
        },
        "inputFormat": {
          "bufferSizeFrames": 480,
          "channels": 1,
          "sampleFormat": "f32",
          "sampleRateHz": 48000
        },
        "inputStream": {
          "callbackViolations": 0,
          "callbacks": 45,
          "frames": 0,
          "issues": 0,
          "lastIssue": "fixture xrun",
          "lastTerminalIssue": null,
          "terminalIssues": 0,
          "xruns": 0
        },
        "metrics": {
          "degraded": false,
          "discontinuities": 0,
          "driftCorrectionPpm": -100,
          "estimatedLatencyMs": 42.5,
          "inferenceLastUs": 0,
          "inferenceP95Us": 0,
          "inferenceP99Us": 1250,
          "inputOverruns": 0,
          "inputPeak": 0.25,
          "inputRingFillFrames": 0,
          "inputRingHighWaterFrames": 0,
          "inputRms": 0.0,
          "missedDeadlines": 0,
          "modelResets": 0,
          "outputPeak": 0.0,
          "outputRingFillFrames": 0,
          "outputRingLowWaterFrames": 0,
          "outputRms": 0.0,
          "outputUnderruns": 0,
          "overloadEvents": 0,
          "resetLastUs": 0,
          "resetTotalUs": 0,
          "skippedInferences": 0
        },
        "model": {
          "algorithmicLatencySamples": 480,
          "channels": 1,
          "frameSamplesPerChannel": 480,
          "id": "DeepFilterNet3_ll",
          "sampleRateHz": 48000
        },
        "modelErrors": 0,
        "outputDevice": null,
        "outputFormat": {
          "bufferSizeFrames": 480,
          "channels": 1,
          "sampleFormat": "f32",
          "sampleRateHz": 48000
        },
        "outputStream": {
          "callbackViolations": 0,
          "callbacks": 0,
          "frames": 0,
          "issues": 0,
          "lastIssue": null,
          "lastTerminalIssue": null,
          "terminalIssues": 0,
          "xruns": 0
        },
        "recoveryAttempts": 0,
        "state": "running",
        "successfulRecoveries": 1,
        "uptimeSeconds": 0.0
      },
      {
        "bypassed": true,
        "config": {
          "denoise": {
            "attenuationLimitDb": null,
            "enabled": true,
            "postFilterEnabled": true
          },
          "destination": {
            "physicalOutput": "fake:output"
          },
          "echoCancellation": false,
          "id": "desktop-incoming",
          "processingMode": "aligned_bypass",
          "source": {
            "systemLoopback": "fake:loopback"
          },
          "targetLatencyMs": 40
        },
        "echoCancellation": null,
        "fault": {
          "code": "device_disconnected",
          "details": "fixture stream 2",
          "message": "Fixture headphones disappeared.",
          "suggestedAction": "Reconnect headphones."
        },
        "inputDevice": null,
        "inputFormat": null,
        "inputStream": {
          "callbackViolations": 0,
          "callbacks": 0,
          "frames": 0,
          "issues": 0,
          "lastIssue": null,
          "lastTerminalIssue": null,
          "terminalIssues": 0,
          "xruns": 0
        },
        "metrics": {
          "degraded": false,
          "discontinuities": 0,
          "driftCorrectionPpm": 0,
          "estimatedLatencyMs": 0.0,
          "inferenceLastUs": 0,
          "inferenceP95Us": 0,
          "inferenceP99Us": 0,
          "inputOverruns": 0,
          "inputPeak": 0.0,
          "inputRingFillFrames": 0,
          "inputRingHighWaterFrames": 0,
          "inputRms": 0.0,
          "missedDeadlines": 0,
          "modelResets": 0,
          "outputPeak": 0.0,
          "outputRingFillFrames": 0,
          "outputRingLowWaterFrames": 0,
          "outputRms": 0.0,
          "outputUnderruns": 0,
          "overloadEvents": 0,
          "resetLastUs": 0,
          "resetTotalUs": 0,
          "skippedInferences": 0
        },
        "model": null,
        "modelErrors": 0,
        "outputDevice": null,
        "outputFormat": null,
        "outputStream": {
          "callbackViolations": 0,
          "callbacks": 0,
          "frames": 0,
          "issues": 0,
          "lastIssue": null,
          "lastTerminalIssue": null,
          "terminalIssues": 0,
          "xruns": 0
        },
        "recoveryAttempts": 0,
        "state": "failed",
        "successfulRecoveries": 0,
        "uptimeSeconds": 0.0
      }
    ],
    "routing": {
      "capabilities": {
        "physicalInput": true,
        "physicalOutput": true,
        "requiresExternalDriver": false,
        "supportsRenderReference": true,
        "systemLoopback": true,
        "virtualMicrophone": true,
        "virtualSpeaker": false
      },
      "endpoints": [
        {
          "ancestry": {
            "complete": true,
            "recursivePaths": []
          },
          "availability": "ready",
          "canCreate": false,
          "captureDeviceId": "fake:virtual-input",
          "id": "fake:virtual-microphone",
          "instructions": [
            "Fixture setup instructions."
          ],
          "name": "Fixture virtual microphone",
          "playbackDeviceId": "fake:virtual-output",
          "setupUrl": "https://example.com/fixture"
        }
      ],
      "loopbackEndpoints": [
        {
          "ancestry": {
            "complete": true,
            "recursivePaths": []
          },
          "availability": "ready",
          "canCreate": false,
          "captureDeviceId": "fake:virtual-input",
          "id": "fake:virtual-microphone",
          "instructions": [
            "Fixture setup instructions."
          ],
          "name": "Fixture virtual microphone",
          "playbackDeviceId": "fake:virtual-output",
          "setupUrl": "https://example.com/fixture"
        }
      ]
    },
    "shuttingDown": false
  },
  "updates": [
    {
      "downloadedBytes": 1024,
      "error": null,
      "phase": "disabled",
      "revision": 0,
      "totalBytes": 2048,
      "version": "0.2.0"
    },
    {
      "downloadedBytes": 1024,
      "error": null,
      "phase": "idle",
      "revision": 1,
      "totalBytes": 2048,
      "version": "0.2.0"
    },
    {
      "downloadedBytes": 1024,
      "error": null,
      "phase": "checking",
      "revision": 2,
      "totalBytes": 2048,
      "version": "0.2.0"
    },
    {
      "downloadedBytes": 1024,
      "error": null,
      "phase": "downloading",
      "revision": 3,
      "totalBytes": 2048,
      "version": "0.2.0"
    },
    {
      "downloadedBytes": 1024,
      "error": null,
      "phase": "ready",
      "revision": 4,
      "totalBytes": 2048,
      "version": "0.2.0"
    },
    {
      "downloadedBytes": 1024,
      "error": null,
      "phase": "installing",
      "revision": 5,
      "totalBytes": 2048,
      "version": "0.2.0"
    },
    {
      "downloadedBytes": 1024,
      "error": "Fixture signature rejected.",
      "phase": "error",
      "revision": 6,
      "totalBytes": 2048,
      "version": "0.2.0"
    }
  ]
} as const;
