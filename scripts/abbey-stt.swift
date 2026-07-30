#!/usr/bin/env swift
// Abbey on-device speech-to-text helper (macOS Speech + AVAudioEngine).
// Built lazily by `abbey voice listen` into $ABBEY_STATE_DIR/bin/abbey-stt.
// Prefers on-device recognition when the OS supports it.

import AVFoundation
import Foundation
import Speech

let args = Array(CommandLine.arguments.dropFirst())
var seconds: Double = 5
var localeId = "en-US"
var i = 0
while i < args.count {
    switch args[i] {
    case "--seconds", "-s":
        i += 1
        if i < args.count { seconds = Double(args[i]) ?? seconds }
    case "--locale", "-l":
        i += 1
        if i < args.count { localeId = args[i] }
    case "-h", "--help":
        fputs("usage: abbey-stt [--seconds N] [--locale en-US]\n", stderr)
        exit(0)
    default:
        break
    }
    i += 1
}
seconds = min(max(seconds, 1), 60)

let auth = DispatchSemaphore(value: 0)
var authStatus = SFSpeechRecognizerAuthorizationStatus.notDetermined
SFSpeechRecognizer.requestAuthorization { status in
    authStatus = status
    auth.signal()
}
auth.wait()
guard authStatus == .authorized else {
    fputs(
        "abbey-stt: authorize Speech Recognition + Microphone in\n"
            + "  System Settings → Privacy & Security\n",
        stderr
    )
    exit(2)
}

guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: localeId)),
      recognizer.isAvailable
else {
    fputs("abbey-stt: recognizer unavailable for locale \(localeId)\n", stderr)
    exit(3)
}

let engine = AVAudioEngine()
let request = SFSpeechAudioBufferRecognitionRequest()
request.shouldReportPartialResults = false
if #available(macOS 13, *) {
    request.requiresOnDeviceRecognition = true
}

let input = engine.inputNode
let format = input.outputFormat(forBus: 0)
guard format.sampleRate > 0 else {
    fputs("abbey-stt: no microphone input (check Mic permission for Terminal/abbey)\n", stderr)
    exit(4)
}

input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
    request.append(buffer)
}

var finalText = ""
let done = DispatchSemaphore(value: 0)
_ = recognizer.recognitionTask(with: request) { result, error in
    if let result {
        finalText = result.bestTranscription.formattedString
        if result.isFinal { done.signal() }
    } else if error != nil {
        done.signal()
    }
}

do {
    try engine.start()
} catch {
    fputs("abbey-stt: engine start failed: \(error)\n", stderr)
    exit(5)
}

fputs("abbey-stt: listening \(Int(seconds))s (on-device when available)…\n", stderr)
Thread.sleep(forTimeInterval: seconds)
request.endAudio()
engine.stop()
input.removeTap(onBus: 0)
_ = done.wait(timeout: .now() + 20)

if finalText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
    fputs("abbey-stt: no speech recognized\n", stderr)
    exit(6)
}
print(finalText)
