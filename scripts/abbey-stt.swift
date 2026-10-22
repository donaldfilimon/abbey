#!/usr/bin/env swift
// Abbey on-device speech-to-text helper (macOS Speech + AVAudioEngine).
// Built lazily by `abbey voice listen` into $ABBEY_STATE_DIR/bin/abbey-stt.
//
// Contract with `src/voice.rs` (`voice::listen`):
//   * stdout carries the transcript and nothing else — the caller trims it and
//     hands it back to whoever asked to listen.
//   * stderr carries human-readable diagnostics.
//   * exit 0 means a transcript was produced; every other exit is a failure.
//
// Recognition is on-device by default and fails closed when the locale has no
// on-device model: the Speech framework silently falls back to Apple's servers
// in that case, and Abbey's ledger claims on-device STT with no cloud speech.
// `--allow-network` opts into that fallback explicitly.
//
// Exit codes:
//   0  transcript produced
//   1  usage error
//   2  Speech Recognition or Microphone permission denied
//   3  recognizer unavailable for the locale
//   4  no microphone input
//   5  audio engine failed to start
//   6  no speech recognized
//   7  recognition failed

import AVFoundation
import Darwin
import Foundation
import Speech

// MARK: - Exit codes

/// `src/voice.rs` reports this code verbatim, so 2...=6 keep the meanings the
/// CLI and the docs already describe.
enum ExitCode: Int32 {
    case ok = 0
    case usage = 1
    case permission = 2
    case recognizer = 3
    case noInput = 4
    case engine = 5
    case noSpeech = 6
    case recognition = 7
}

// MARK: - Channels

/// stdout is reserved for the transcript; diagnostics go to stderr.
func log(_ message: String) {
    FileHandle.standardError.write(Data((message + "\n").utf8))
}

func fail(_ code: ExitCode, _ message: String) -> Never {
    log(message)
    exit(code.rawValue)
}

/// A lock-protected box for values that cross a framework callback boundary.
final class Locked<Value> {
    private let lock = NSLock()
    private var value: Value

    init(_ value: Value) {
        self.value = value
    }

    func withLock<Result>(_ body: (inout Value) -> Result) -> Result {
        lock.lock()
        defer { lock.unlock() }
        return body(&value)
    }
}

// MARK: - Session state

/// State shared between the main thread and the Speech callback queue. The
/// callback fires on a framework queue, so every field is guarded by the
/// condition's lock and every wait re-checks the predicates.
final class Session {
    private let condition = NSCondition()
    private var text = ""
    private var isFinal = false
    private var failure: Error?
    private var stopRequested = false
    private var announced = ""

    func record(_ transcription: String, final: Bool) {
        condition.lock()
        defer { condition.unlock() }
        text = transcription
        isFinal = final
        condition.broadcast()
    }

    func recordFailure(_ error: Error) {
        condition.lock()
        defer { condition.unlock() }
        failure = error
        condition.broadcast()
    }

    /// Ctrl-C ends the capture but keeps whatever was heard, so the transcript
    /// is not lost the way it is when the process is simply killed.
    func requestStop() {
        condition.lock()
        defer { condition.unlock() }
        stopRequested = true
        condition.broadcast()
    }

    /// Interim transcripts are echoed only when they change.
    func shouldAnnounce(_ transcription: String) -> Bool {
        condition.lock()
        defer { condition.unlock() }
        if transcription == announced { return false }
        announced = transcription
        return true
    }

    var snapshot: (text: String, isFinal: Bool, failure: Error?) {
        condition.lock()
        defer { condition.unlock() }
        return (text, isFinal, failure)
    }

    var stopped: Bool {
        condition.lock()
        defer { condition.unlock() }
        return stopRequested
    }

    /// Waits for the capture window to close, a stop request, or a result.
    func wait(until deadline: Date) {
        condition.lock()
        defer { condition.unlock() }
        while !stopRequested && !isFinal && failure == nil {
            let remaining = deadline.timeIntervalSinceNow
            if remaining <= 0 { return }
            _ = condition.wait(until: Date(timeIntervalSinceNow: min(remaining, 0.2)))
        }
    }

    /// Bounded grace period for the final transcription after `endAudio()`.
    func waitForFinal(timeout: TimeInterval) {
        condition.lock()
        defer { condition.unlock() }
        let deadline = Date().addingTimeInterval(timeout)
        while !isFinal && failure == nil && Date() < deadline {
            _ = condition.wait(until: Date(timeIntervalSinceNow: 0.1))
        }
    }
}

/// Every mutable field is guarded by the condition's lock, so the session can
/// be shared with the framework callbacks and the signal source.
extension Session: @unchecked Sendable {}

// MARK: - Options

struct Options {
    var seconds: Double = 5
    var localeId: String = "en-US"
    /// Opt in to Apple's networked recognition when the locale has no on-device
    /// model. Off by default: the fallback sends the recording off this
    /// machine, which the on-device STT claim does not cover.
    var allowNetwork = false
    /// Echo interim transcriptions to stderr while listening.
    var showPartials = false
}

let usage = """
usage: abbey-stt [--seconds N] [--locale ID] [--allow-network] [--partial]

  -s, --seconds N   capture window in seconds (clamped to 1...60, default 5)
  -l, --locale ID   BCP-47 locale for recognition (default en-US)
      --allow-network
                    allow Apple's networked recognition when the locale has no
                    on-device model; the recording then leaves this machine
      --partial     echo interim transcriptions to stderr while listening
  -h, --help        show this help
"""

func parseOptions(_ args: [String]) -> Options {
    var options = Options()
    var index = 0
    while index < args.count {
        let argument = args[index]
        switch argument {
        case "--seconds", "-s":
            index += 1
            guard index < args.count, let value = Double(args[index]), value.isFinite else {
                fail(.usage, "abbey-stt: --seconds needs a number\n\n\(usage)")
            }
            options.seconds = value
        case "--locale", "-l":
            index += 1
            guard index < args.count, !args[index].isEmpty else {
                fail(.usage, "abbey-stt: --locale needs a locale id\n\n\(usage)")
            }
            options.localeId = args[index]
        case "--allow-network":
            options.allowNetwork = true
        case "--partial":
            options.showPartials = true
        case "-h", "--help":
            log(usage)
            exit(ExitCode.ok.rawValue)
        default:
            fail(.usage, "abbey-stt: unknown argument '\(argument)'\n\n\(usage)")
        }
        index += 1
    }
    options.seconds = min(max(options.seconds, 1), 60)
    return options
}

// MARK: - Permissions

func describe(_ status: SFSpeechRecognizerAuthorizationStatus) -> String {
    switch status {
    case .authorized: return "granted"
    case .denied: return "denied"
    case .restricted: return "restricted (parental controls or device policy)"
    case .notDetermined: return "not requested"
    @unknown default: return "unknown"
    }
}

/// Asks for Speech Recognition access and waits for the answer. The callback
/// runs on a framework queue, so the status crosses a thread boundary through
/// a lock rather than a captured `var`.
func requestSpeechAuthorization() -> SFSpeechRecognizerAuthorizationStatus {
    let semaphore = DispatchSemaphore(value: 0)
    let status = Locked<SFSpeechRecognizerAuthorizationStatus>(.notDetermined)
    SFSpeechRecognizer.requestAuthorization { granted in
        status.withLock { $0 = granted }
        semaphore.signal()
    }
    semaphore.wait()
    return status.withLock { $0 }
}

/// Checks microphone access before the engine starts, so a denial is reported
/// as a denial instead of surfacing as an opaque engine failure.
func requestMicrophoneAccess() -> Bool {
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized:
        return true
    case .notDetermined:
        let semaphore = DispatchSemaphore(value: 0)
        let granted = Locked(false)
        AVCaptureDevice.requestAccess(for: .audio) { allowed in
            granted.withLock { $0 = allowed }
            semaphore.signal()
        }
        semaphore.wait()
        return granted.withLock { $0 }
    case .denied, .restricted:
        return false
    @unknown default:
        return false
    }
}

// MARK: - Recognizer

/// Speech services can take a moment to come up after launch, so a recognizer
/// that is not yet available is given a short grace period before it is
/// declared unusable.
func waitForAvailability(_ recognizer: SFSpeechRecognizer, timeout: TimeInterval = 2.0) -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if recognizer.isAvailable { return true }
        Thread.sleep(forTimeInterval: 0.1)
    }
    return recognizer.isAvailable
}

func describe(_ error: Error) -> String {
    let nsError = error as NSError
    return "\(nsError.localizedDescription) [\(nsError.domain) \(nsError.code)]"
}

// MARK: - Capture

/// Attaches the capture tap to the engine's input.
///
/// `installTap(onBus:bufferSize:format:block:)` is deprecated in macOS 27 in
/// favour of `installTapOnBus:bufferSize:format:error:block:`, but that
/// replacement is marked `NS_REFINED_FOR_SWIFT` with no `swift_name`, so it is
/// not imported into Swift at all — the deprecated spelling is the only one a
/// Swift helper can call. The build in `src/voice.rs` pins a deployment target
/// below 27.0, which is also the version the rest of this script needs, so the
/// deprecation stays out of the build.
func installCaptureTap(
    on node: AVAudioNode,
    format: AVAudioFormat,
    handler: @escaping (AVAudioPCMBuffer) -> Void
) {
    node.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
        handler(buffer)
    }
}

/// Routes SIGINT and SIGTERM to `session.requestStop()`.
///
/// The source runs on a background queue on purpose: the capture waits on the
/// main thread, so a main-queue source would never be serviced while the
/// process is listening.
func installInterruptHandlers(_ session: Session) -> [DispatchSourceSignal] {
    var sources: [DispatchSourceSignal] = []
    let queue = DispatchQueue(label: "abbey-stt.signals")
    for number in [SIGINT, SIGTERM] {
        // Ignore the default disposition so the process survives long enough to
        // stop the engine and print the transcript.
        signal(number, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: number, queue: queue)
        source.setEventHandler { session.requestStop() }
        source.resume()
        sources.append(source)
    }
    return sources
}

func run(_ options: Options) -> ExitCode {
    let authStatus = requestSpeechAuthorization()
    guard authStatus == .authorized else {
        fail(
            .permission,
            """
            abbey-stt: Speech Recognition permission is \(describe(authStatus))
              System Settings → Privacy & Security → Speech Recognition
              grant it to the app that launched this process (usually Terminal)
            """
        )
    }

    guard requestMicrophoneAccess() else {
        fail(
            .permission,
            """
            abbey-stt: Microphone permission is denied
              System Settings → Privacy & Security → Microphone
              grant it to the app that launched this process (usually Terminal)
            """
        )
    }

    guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: options.localeId)) else {
        fail(.recognizer, "abbey-stt: no recognizer for locale \(options.localeId)")
    }
    guard waitForAvailability(recognizer) else {
        fail(
            .recognizer,
            "abbey-stt: recognizer unavailable for locale \(options.localeId) "
                + "(speech services are not ready)"
        )
    }

    let request = SFSpeechAudioBufferRecognitionRequest()
    // Interim results are always captured, so an interrupted or timed-out
    // capture still has something to report; they are only echoed on request.
    request.shouldReportPartialResults = true
    request.taskHint = .dictation
    if #available(macOS 13, *) {
        request.addsPunctuation = true
    }

    // On-device recognition is only honoured when the recognizer supports it;
    // otherwise the framework silently uses the network. Failing closed keeps
    // the on-device claim honest, and the error names the way out.
    let onDeviceAvailable = recognizer.supportsOnDeviceRecognition
    if onDeviceAvailable {
        request.requiresOnDeviceRecognition = true
    } else if options.allowNetwork {
        log(
            "abbey-stt: warning — no on-device model for \(options.localeId); "
                + "the recording is sent to Apple for recognition"
        )
    } else {
        fail(
            .recognizer,
            """
            abbey-stt: no on-device recognition model for \(options.localeId)
              the Speech framework would fall back to Apple's servers, so this
              helper refuses by default; install the on-device dictation assets
              (System Settings → Keyboard → Dictation) or pass --allow-network
            """
        )
    }

    let engine = AVAudioEngine()
    let input = engine.inputNode
    // The input node's *output* format is the engine's processing format; the
    // hardware input format can be zero when no microphone is attached.
    let format = input.outputFormat(forBus: 0)
    guard format.sampleRate > 0, format.channelCount > 0 else {
        fail(
            .noInput,
            "abbey-stt: no microphone input "
                + "(check the Mic permission for the app that launched this process)"
        )
    }

    installCaptureTap(on: input, format: format) { buffer in
        request.append(buffer)
    }

    let session = Session()

    // Installed after the permission prompts so Ctrl-C still terminates the
    // process normally while a prompt is on screen.
    let interruptSources = installInterruptHandlers(session)
    defer { interruptSources.forEach { $0.cancel() } }

    // The task is held for the lifetime of the capture so it cannot be
    // released while audio is still being appended.
    let task = recognizer.recognitionTask(with: request) { result, error in
        if let result {
            let transcription = result.bestTranscription.formattedString
            session.record(transcription, final: result.isFinal)
            if options.showPartials, session.shouldAnnounce(transcription) {
                log("abbey-stt: … \(transcription)")
            }
        } else if let error {
            session.recordFailure(error)
        }
    }

    do {
        try engine.start()
    } catch {
        input.removeTap(onBus: 0)
        fail(.engine, "abbey-stt: audio engine failed to start: \(describe(error))")
    }

    let mode = onDeviceAvailable ? "on-device" : "networked"
    log("abbey-stt: listening \(Int(options.seconds))s (\(mode))…")

    session.wait(until: Date().addingTimeInterval(options.seconds))

    request.endAudio()
    engine.stop()
    input.removeTap(onBus: 0)
    session.waitForFinal(timeout: 5.0)
    task.cancel()

    let snapshot = session.snapshot
    let text = snapshot.text.trimmingCharacters(in: .whitespacesAndNewlines)
    if !text.isEmpty {
        print(text)
        return .ok
    }
    if let failure = snapshot.failure {
        var message = "abbey-stt: recognition failed: \(describe(failure))"
        if onDeviceAvailable {
            message += "\n  on-device dictation assets for \(options.localeId) may be missing"
            message += " (System Settings → Keyboard → Dictation)"
        }
        fail(.recognition, message)
    }
    if session.stopped {
        fail(.noSpeech, "abbey-stt: interrupted before any speech was recognized")
    }
    fail(.noSpeech, "abbey-stt: no speech recognized")
}

// MARK: - Entry point

let options = parseOptions(Array(CommandLine.arguments.dropFirst()))
exit(run(options).rawValue)
