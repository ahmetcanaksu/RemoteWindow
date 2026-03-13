import CoreGraphics
import CoreMedia
import CoreVideo
import Darwin
import Foundation
import IOSurface
import ScreenCaptureKit

private let bridgeErrorDomain = "RemoteWindowScreenCapture"

private func bridgeLog(_ message: String) {
    fputs("[swift-bridge] \(message)\n", stderr)
}

private func writeError(_ message: String, _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) {
    guard let outError = outError else {
        return
    }

    outError.pointee = strdup(message)
}

@available(macOS 12.3, *)
private final class FrameOutput: NSObject, SCStreamOutput, SCStreamDelegate {
    private let condition = NSCondition()
    private var latestFrame: Data?
    private var latestSerial: UInt64 = 0
    private var deliveredSerial: UInt64 = 0
    private var lastError: String?

    func waitForFrame(timeoutMs: UInt32) throws -> Data {
        let timeoutDate = Date().addingTimeInterval(Double(timeoutMs) / 1000.0)

        condition.lock()
        defer { condition.unlock() }

        while latestFrame == nil || deliveredSerial == latestSerial {
            if let lastError = lastError {
                throw NSError(
                    domain: bridgeErrorDomain,
                    code: 1,
                    userInfo: [NSLocalizedDescriptionKey: lastError]
                )
            }

            if !condition.wait(until: timeoutDate) {
                break
            }
        }

        if let lastError = lastError {
            throw NSError(
                domain: bridgeErrorDomain,
                code: 2,
                userInfo: [NSLocalizedDescriptionKey: lastError]
            )
        }

        guard let latestFrame = latestFrame else {
            throw NSError(
                domain: bridgeErrorDomain,
                code: 3,
                userInfo: [NSLocalizedDescriptionKey: "timed out waiting for the first screen frame"]
            )
        }

        deliveredSerial = latestSerial
        return latestFrame
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of outputType: SCStreamOutputType) {
        guard outputType == .screen else {
            return
        }

        guard CMSampleBufferIsValid(sampleBuffer),
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false)
                as? [[SCStreamFrameInfo: Any]],
              let attachment = attachments.first,
              let rawStatus = attachment[.status] as? Int,
              let status = SCFrameStatus(rawValue: rawStatus),
              status == .complete,
              let pixelBuffer = sampleBuffer.imageBuffer
        else {
            return
        }

        guard let frameData = Self.makePackedFrameData(from: pixelBuffer) else {
            return
        }

        condition.lock()
        latestFrame = frameData
        latestSerial += 1
        condition.broadcast()
        condition.unlock()
    }

    func stream(_ stream: SCStream, didStopWithError error: any Error) {
        condition.lock()
        lastError = error.localizedDescription
        condition.broadcast()
        condition.unlock()
    }

    private static func makePackedFrameData(from pixelBuffer: CVImageBuffer) -> Data? {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
            return nil
        }

        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)
        let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
        let packedBytesPerRow = width * 4

        if bytesPerRow == packedBytesPerRow {
            return Data(bytes: baseAddress, count: packedBytesPerRow * height)
        }

        var packed = Data(count: packedBytesPerRow * height)
        packed.withUnsafeMutableBytes { destination in
            guard let destinationBase = destination.baseAddress else {
                return
            }

            for row in 0 ..< height {
                let sourceRow = baseAddress.advanced(by: row * bytesPerRow)
                let destinationRow = destinationBase.advanced(by: row * packedBytesPerRow)
                memcpy(destinationRow, sourceRow, packedBytesPerRow)
            }
        }

        return packed
    }
}

@available(macOS 12.3, *)
private final class ScreenCaptureBridge {
    let width: Int
    let height: Int

    private let output = FrameOutput()
    private let sampleQueue = DispatchQueue(label: "RemoteWindow.ScreenCaptureKit")
    private let stream: SCStream

    init(displayIndex: Int) throws {
        bridgeLog("querying shareable content")
        let shareableContent = try Self.await(timeoutSeconds: 8) {
            try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        }
        bridgeLog("shareable content query complete")

        guard !shareableContent.displays.isEmpty else {
            throw NSError(
                domain: bridgeErrorDomain,
                code: 4,
                userInfo: [NSLocalizedDescriptionKey: "ScreenCaptureKit reported no shareable displays"]
            )
        }

        let safeIndex = max(0, min(displayIndex, shareableContent.displays.count - 1))
        let display = shareableContent.displays[safeIndex]

        width = display.width
        height = display.height

        let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
        let configuration = SCStreamConfiguration()
        configuration.width = width
        configuration.height = height
        configuration.pixelFormat = OSType(kCVPixelFormatType_32BGRA)
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 15)
        configuration.queueDepth = 2
        configuration.showsCursor = true

        stream = SCStream(filter: filter, configuration: configuration, delegate: output)
        try stream.addStreamOutput(output, type: .screen, sampleHandlerQueue: sampleQueue)
        bridgeLog("starting ScreenCaptureKit stream")
        try Self.await(timeoutSeconds: 8) {
            try await self.stream.startCapture()
        }
        bridgeLog("ScreenCaptureKit stream started")
    }

    deinit {
        Self.awaitIgnoringErrors {
            try await self.stream.stopCapture()
        }
    }

    func captureFrame(timeoutMs: UInt32) throws -> Data {
        try output.waitForFrame(timeoutMs: timeoutMs)
    }

    private static func await<T>(timeoutSeconds: TimeInterval, _ operation: @escaping () async throws -> T) throws -> T {
        let semaphore = DispatchSemaphore(value: 0)
        var result: Result<T, Error>?

        Task {
            do {
                result = .success(try await operation())
            } catch {
                result = .failure(error)
            }
            semaphore.signal()
        }

        if semaphore.wait(timeout: .now() + timeoutSeconds) == .timedOut {
            throw NSError(
                domain: bridgeErrorDomain,
                code: 5,
                userInfo: [NSLocalizedDescriptionKey: "timed out waiting for ScreenCaptureKit async operation"]
            )
        }

        return try result!.get()
    }

    private static func awaitIgnoringErrors(_ operation: @escaping () async throws -> Void) {
        let semaphore = DispatchSemaphore(value: 0)

        Task {
            _ = try? await operation()
            semaphore.signal()
        }

        semaphore.wait()
    }
}

@_cdecl("rw_sc_create")
public func rw_sc_create(
    _ displayIndex: UInt32,
    _ outWidth: UnsafeMutablePointer<UInt32>?,
    _ outHeight: UnsafeMutablePointer<UInt32>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard #available(macOS 12.3, *) else {
        writeError("ScreenCaptureKit requires macOS 12.3 or newer", outError)
        return nil
    }

    do {
        bridgeLog("creating bridge handle")
        let capturer = try ScreenCaptureBridge(displayIndex: Int(displayIndex))
        outWidth?.pointee = UInt32(capturer.width)
        outHeight?.pointee = UInt32(capturer.height)
        bridgeLog("bridge handle ready at \(capturer.width)x\(capturer.height)")
        return Unmanaged.passRetained(capturer).toOpaque()
    } catch {
        bridgeLog("create failed: \(error.localizedDescription)")
        writeError(error.localizedDescription, outError)
        return nil
    }
}

@_cdecl("rw_sc_capture_frame")
public func rw_sc_capture_frame(
    _ handle: UnsafeMutableRawPointer?,
    _ timeoutMs: UInt32,
    _ outLength: UnsafeMutablePointer<Int>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard #available(macOS 12.3, *) else {
        writeError("ScreenCaptureKit requires macOS 12.3 or newer", outError)
        return nil
    }

    guard let handle = handle else {
        writeError("capture handle was null", outError)
        return nil
    }

    do {
        let capturer = Unmanaged<ScreenCaptureBridge>.fromOpaque(handle).takeUnretainedValue()
        let data = try capturer.captureFrame(timeoutMs: timeoutMs)
        guard let buffer = malloc(data.count) else {
            writeError("failed to allocate a frame buffer", outError)
            return nil
        }

        data.withUnsafeBytes { bytes in
            if let source = bytes.baseAddress {
                memcpy(buffer, source, data.count)
            }
        }

        outLength?.pointee = data.count
        return buffer
    } catch {
        writeError(error.localizedDescription, outError)
        return nil
    }
}

@_cdecl("rw_sc_destroy")
public func rw_sc_destroy(_ handle: UnsafeMutableRawPointer?) {
    guard let handle = handle else {
        return
    }

    if #available(macOS 12.3, *) {
        Unmanaged<ScreenCaptureBridge>.fromOpaque(handle).release()
    }
}

@_cdecl("rw_sc_free_frame")
public func rw_sc_free_frame(_ frame: UnsafeMutableRawPointer?) {
    guard let frame = frame else {
        return
    }

    free(frame)
}

@_cdecl("rw_sc_free_error")
public func rw_sc_free_error(_ error: UnsafeMutablePointer<CChar>?) {
    guard let error = error else {
        return
    }

    free(error)
}