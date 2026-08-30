import Foundation
import MediaPlayer
import AVFoundation
import SwiftRs
import Tauri
import UIKit

// MARK: - Command argument structs

struct UpdateStateArgs: Decodable {
    let title: String?
    let artist: String?
    let album: String?
    let artworkUrl: String?
    let duration: Double?
    let position: Double?
    let playbackSpeed: Double?
    let isPlaying: Bool?
    /// Preparing the track rather than paused. iOS has no buffering state in
    /// `MPNowPlayingInfoCenter`, so this only pins the reported playback rate
    /// to 0 — it exists to keep both native halves speaking the same protocol
    /// as `MediaState` on the Rust side.
    let isLoading: Bool?
    let canPrev: Bool?
    let canNext: Bool?
    let canSeek: Bool?
}

struct UpdateTimelineArgs: Decodable {
    let position: Double?
    let duration: Double?
    let playbackSpeed: Double?
}

struct SetEventHandlerArgs: Decodable {
    let handler: Channel
}

// MARK: - Event payload

struct MediaActionPayload: Encodable {
    let action: String
    let seekPosition: Double?
}

// MARK: - Plugin

class MediaSessionPlugin: Plugin {
    private let tag = "plugin/media-session"

    // Merge state — omitted fields in updateState keep their previous values
    private var currentTitle: String = ""
    private var currentArtist: String = ""
    private var currentAlbum: String = ""
    private var currentDuration: Double = 0.0
    private var currentPosition: Double = 0.0
    private var currentPlaybackSpeed: Double = 1.0
    private var currentIsPlaying: Bool = false
    private var currentIsLoading: Bool = false
    private var currentCanPrev: Bool = false
    private var currentCanNext: Bool = false
    private var currentCanSeek: Bool = true

    // Artwork cache
    private var cachedArtworkUrl: String?
    private var cachedArtworkImage: UIImage?
    private var artworkDownloadTask: URLSessionDataTask?
    private var artworkDownloadRequestId: UUID?

    // Event handler channel (Rust → native communication)
    private var eventChannel: Channel?

    // Session state
    private var isSessionActive: Bool = false

    // Notification observers
    private var interruptionObserver: NSObjectProtocol?
    private var routeChangeObserver: NSObjectProtocol?

    private let maxArtworkSize: CGFloat = 512

    // MARK: - Commands

    @objc public func initialize(_ invoke: Invoke) throws {
        NSLog("%@: initialize", tag)
        setupAudioSession()
        setupRemoteCommands()
        isSessionActive = true
        invoke.resolve()
    }

    @objc public func setEventHandler(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SetEventHandlerArgs.self)
        self.eventChannel = args.handler
        NSLog("%@: setEventHandler: event channel registered", tag)
        invoke.resolve()
    }

    @objc public func updateState(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(UpdateStateArgs.self)

        if !isSessionActive {
            setupAudioSession()
            setupRemoteCommands()
            isSessionActive = true
        }

        // Merge incoming args with stored state
        if let title = args.title?.trimmingCharacters(in: .whitespaces) { currentTitle = title }
        if let artist = args.artist?.trimmingCharacters(in: .whitespaces) { currentArtist = artist }
        if let album = args.album?.trimmingCharacters(in: .whitespaces) { currentAlbum = album }
        if let duration = args.duration { currentDuration = duration }
        if let position = args.position { currentPosition = position }
        if let playbackSpeed = args.playbackSpeed { currentPlaybackSpeed = playbackSpeed }
        if let isPlaying = args.isPlaying { currentIsPlaying = isPlaying }
        if let isLoading = args.isLoading { currentIsLoading = isLoading }
        if let canPrev = args.canPrev { currentCanPrev = canPrev }
        if let canNext = args.canNext { currentCanNext = canNext }
        if let canSeek = args.canSeek { currentCanSeek = canSeek }

        if let artworkUrl = args.artworkUrl {
            if artworkUrl.isEmpty {
                NSLog("%@: clearing artwork (URL)", tag)
                artworkDownloadRequestId = nil
                artworkDownloadTask?.cancel()
                artworkDownloadTask = nil
                cachedArtworkImage = fallbackArtworkImage()
                cachedArtworkUrl = nil
            } else if artworkUrl != cachedArtworkUrl {
                NSLog("%@: downloading artwork from URL", tag)
                cachedArtworkUrl = artworkUrl
                downloadAndApplyArtwork(url: artworkUrl)
            }
        }

        // Update now playing info and remote command targets
        updateNowPlayingInfo()
        updateRemoteCommandStates()

        NSLog("%@: updateState applied -> \"%@\" by %@, playing=%d, pos=%.1fs/%.1fs",
              tag, currentTitle, currentArtist,
              currentIsPlaying ? 1 : 0, currentPosition, currentDuration)

        invoke.resolve()
    }

    @objc public func updateTimeline(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(UpdateTimelineArgs.self)

        guard isSessionActive else {
            invoke.reject("media session not initialized — call updateState first")
            return
        }

        if let position = args.position { currentPosition = position }
        if let duration = args.duration { currentDuration = duration }
        if let playbackSpeed = args.playbackSpeed { currentPlaybackSpeed = playbackSpeed }

        updateNowPlayingInfo()
        invoke.resolve()
    }

    @objc public func clear(_ invoke: Invoke) throws {
        NSLog("%@: clear: releasing session", tag)
        releaseSession()
        invoke.resolve()
    }

    // MARK: - Audio session

    private func setupAudioSession() {
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .default)
            try session.setActive(true)
            UIApplication.shared.beginReceivingRemoteControlEvents()
            setupInterruptionHandling()
            NSLog("%@: audio session configured for playback", tag)
        } catch {
            NSLog("%@: audio session setup failed: %@", tag, error.localizedDescription)
        }
    }

    // MARK: - Interruption & route change handling

    private func setupInterruptionHandling() {
        // Remove any existing observers before adding new ones
        if let obs = interruptionObserver { NotificationCenter.default.removeObserver(obs) }
        if let obs = routeChangeObserver { NotificationCenter.default.removeObserver(obs) }

        interruptionObserver = NotificationCenter.default.addObserver(
            forName: AVAudioSession.interruptionNotification,
            object: nil,
            queue: .main
        ) { [weak self] notification in
            self?.handleAudioInterruption(notification)
        }

        routeChangeObserver = NotificationCenter.default.addObserver(
            forName: AVAudioSession.routeChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] notification in
            self?.handleRouteChange(notification)
        }

        NSLog("%@: interruption and route change observers registered", tag)
    }

    private func handleAudioInterruption(_ notification: Notification) {
        guard let userInfo = notification.userInfo,
              let typeValue = userInfo[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: typeValue) else { return }

        switch type {
        case .began:
            NSLog("%@: audio interrupted (call/alarm) — emitting pause", tag)
            emitAction("pause")

        case .ended:
            let options: AVAudioSession.InterruptionOptions
            if let optValue = userInfo[AVAudioSessionInterruptionOptionKey] as? UInt {
                options = AVAudioSession.InterruptionOptions(rawValue: optValue)
            } else {
                options = []
            }
            if options.contains(.shouldResume) {
                NSLog("%@: interruption ended, shouldResume — reactivating session and emitting play", tag)
                try? AVAudioSession.sharedInstance().setActive(true)
                emitAction("play")
            } else {
                NSLog("%@: interruption ended, no resume requested", tag)
            }

        @unknown default:
            break
        }
    }

    private func handleRouteChange(_ notification: Notification) {
        guard let userInfo = notification.userInfo,
              let reasonValue = userInfo[AVAudioSessionRouteChangeReasonKey] as? UInt,
              let reason = AVAudioSession.RouteChangeReason(rawValue: reasonValue) else { return }

        if reason == .oldDeviceUnavailable {
            NSLog("%@: audio route changed — headphones/BT disconnected, emitting pause", tag)
            emitAction("pause")
        }
    }

    // MARK: - Remote Command Center

    private func setupRemoteCommands() {
        let commandCenter = MPRemoteCommandCenter.shared()

        commandCenter.playCommand.addTarget { [weak self] _ in
            self?.emitAction("play")
            return .success
        }

        commandCenter.pauseCommand.addTarget { [weak self] _ in
            self?.emitAction("pause")
            return .success
        }

        commandCenter.stopCommand.addTarget { [weak self] _ in
            self?.emitAction("stop")
            return .success
        }

        commandCenter.nextTrackCommand.addTarget { [weak self] _ in
            self?.emitAction("next")
            return .success
        }

        commandCenter.previousTrackCommand.addTarget { [weak self] _ in
            self?.emitAction("previous")
            return .success
        }

        commandCenter.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let positionEvent = event as? MPChangePlaybackPositionCommandEvent else {
                return .commandFailed
            }
            self?.emitSeek(positionSeconds: positionEvent.positionTime)
            return .success
        }

        // Toggle play/pause (for headphone button)
        commandCenter.togglePlayPauseCommand.addTarget { [weak self] _ in
            guard let self = self else { return .commandFailed }
            if self.currentIsPlaying {
                self.emitAction("pause")
            } else {
                self.emitAction("play")
            }
            return .success
        }

        updateRemoteCommandStates()
    }

    private func updateRemoteCommandStates() {
        let commandCenter = MPRemoteCommandCenter.shared()

        commandCenter.playCommand.isEnabled = true
        commandCenter.pauseCommand.isEnabled = true
        commandCenter.stopCommand.isEnabled = true
        commandCenter.togglePlayPauseCommand.isEnabled = true

        commandCenter.nextTrackCommand.isEnabled = currentCanNext
        commandCenter.previousTrackCommand.isEnabled = currentCanPrev
        commandCenter.changePlaybackPositionCommand.isEnabled = currentCanSeek
    }

    // MARK: - Now Playing Info

    private func updateNowPlayingInfo() {
        var info = [String: Any]()

        if !currentTitle.isEmpty {
            info[MPMediaItemPropertyTitle] = currentTitle
        }
        if !currentArtist.isEmpty {
            info[MPMediaItemPropertyArtist] = currentArtist
        }
        if !currentAlbum.isEmpty {
            info[MPMediaItemPropertyAlbumTitle] = currentAlbum
        }
        if currentDuration > 0 {
            info[MPMediaItemPropertyPlaybackDuration] = currentDuration
        }

        info[MPNowPlayingInfoPropertyElapsedPlaybackTime] = currentPosition
        info[MPNowPlayingInfoPropertyPlaybackRate] =
            (currentIsPlaying && !currentIsLoading) ? currentPlaybackSpeed : 0.0
        info[MPNowPlayingInfoPropertyDefaultPlaybackRate] = currentPlaybackSpeed

        if let image = cachedArtworkImage {
            let artworkItem = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
            info[MPMediaItemPropertyArtwork] = artworkItem
        }

        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    // MARK: - Events → Rust (via Channel)

    private func emitAction(_ action: String) {
        NSLog("%@: emit action=\"%@\"", tag, action)
        let payload = MediaActionPayload(action: action, seekPosition: nil)
        DispatchQueue.main.async {
            self.eventChannel?.send(payload)
        }
    }

    private func emitSeek(positionSeconds: Double) {
        NSLog("%@: emit action=\"seek\", seekPosition=%.1fs", tag, positionSeconds)
        let payload = MediaActionPayload(action: "seek", seekPosition: positionSeconds)
        DispatchQueue.main.async {
            self.eventChannel?.send(payload)
        }
    }

    // MARK: - Artwork download

    private func downloadAndApplyArtwork(url: String) {
        guard let imageUrl = URL(string: url) else {
            NSLog("%@: invalid artwork URL: %@", tag, url)
            return
        }

        let requestId = UUID()
        artworkDownloadRequestId = requestId
        artworkDownloadTask?.cancel()
        let capturedUrl = url
        let task = URLSession.shared.dataTask(with: imageUrl) { [weak self] data, _, error in
            guard let self = self else { return }

            guard self.artworkDownloadRequestId == requestId else {
                return
            }

            if let error = error {
                NSLog("%@: artwork download failed: %@", self.tag, error.localizedDescription)
                DispatchQueue.main.async {
                    guard self.artworkDownloadRequestId == requestId else { return }
                    self.artworkDownloadRequestId = nil
                    self.artworkDownloadTask = nil
                    self.cachedArtworkUrl = nil
                    self.cachedArtworkImage = self.fallbackArtworkImage()
                    self.updateNowPlayingInfo()
                }
                return
            }

            guard let data = data, let image = UIImage(data: data) else {
                NSLog("%@: artwork decode failed for %@", self.tag, capturedUrl)
                DispatchQueue.main.async {
                    guard self.artworkDownloadRequestId == requestId else { return }
                    self.artworkDownloadRequestId = nil
                    self.artworkDownloadTask = nil
                    self.cachedArtworkUrl = nil
                    self.cachedArtworkImage = self.fallbackArtworkImage()
                    self.updateNowPlayingInfo()
                }
                return
            }

            let downsampled = self.downsample(image: image, maxSize: self.maxArtworkSize)
            NSLog("%@: downloaded artwork %.0fx%.0f from %@", self.tag,
                  downsampled.size.width, downsampled.size.height, capturedUrl)

            DispatchQueue.main.async {
                guard self.artworkDownloadRequestId == requestId else {
                    return
                }
                // Only apply if URL hasn't changed since download started
                guard self.cachedArtworkUrl == capturedUrl else {
                    return
                }
                self.artworkDownloadRequestId = nil
                self.artworkDownloadTask = nil
                self.cachedArtworkImage = downsampled
                self.updateNowPlayingInfo()
            }
        }
        artworkDownloadTask = task
        task.resume()
    }

    // MARK: - Image helpers

    private func downsample(image: UIImage, maxSize: CGFloat) -> UIImage {
        let width = image.size.width
        let height = image.size.height

        guard width > maxSize || height > maxSize else { return image }

        let scale: CGFloat
        if width > height {
            scale = maxSize / width
        } else {
            scale = maxSize / height
        }

        let newSize = CGSize(width: width * scale, height: height * scale)
        let renderer = UIGraphicsImageRenderer(size: newSize)
        return renderer.image { _ in
            image.draw(in: CGRect(origin: .zero, size: newSize))
        }
    }

    // MARK: - Cleanup

    private func releaseSession() {
        // Remove system notification observers
        if let obs = interruptionObserver {
            NotificationCenter.default.removeObserver(obs)
            interruptionObserver = nil
        }
        if let obs = routeChangeObserver {
            NotificationCenter.default.removeObserver(obs)
            routeChangeObserver = nil
        }

        let commandCenter = MPRemoteCommandCenter.shared()
        commandCenter.playCommand.removeTarget(nil)
        commandCenter.pauseCommand.removeTarget(nil)
        commandCenter.stopCommand.removeTarget(nil)
        commandCenter.nextTrackCommand.removeTarget(nil)
        commandCenter.previousTrackCommand.removeTarget(nil)
        commandCenter.changePlaybackPositionCommand.removeTarget(nil)
        commandCenter.togglePlayPauseCommand.removeTarget(nil)

        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil

        do {
            try AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        } catch {
            NSLog("%@: audio session deactivation failed: %@", tag, error.localizedDescription)
        }

        UIApplication.shared.endReceivingRemoteControlEvents()

        currentTitle = ""
        currentArtist = ""
        currentAlbum = ""
        currentDuration = 0.0
        currentPosition = 0.0
        currentPlaybackSpeed = 1.0
        currentIsPlaying = false
        currentIsLoading = false
        currentCanPrev = false
        currentCanNext = false
        currentCanSeek = true

        cachedArtworkImage = nil
        cachedArtworkUrl = nil
        artworkDownloadRequestId = nil
        artworkDownloadTask?.cancel()
        artworkDownloadTask = nil

        isSessionActive = false
    }

    private func fallbackArtworkImage() -> UIImage? {
        if let image = UIImage(named: "icon") {
            return downsample(image: image, maxSize: maxArtworkSize)
        }

        guard
            let icons = Bundle.main.infoDictionary?["CFBundleIcons"] as? [String: Any],
            let primary = icons["CFBundlePrimaryIcon"] as? [String: Any],
            let files = primary["CFBundleIconFiles"] as? [String],
            let last = files.last,
            let icon = UIImage(named: last)
        else {
            return nil
        }

        return downsample(image: icon, maxSize: maxArtworkSize)
    }
}

// MARK: - Plugin init

@_cdecl("init_plugin_media_session")
func initPlugin() -> Plugin {
    return MediaSessionPlugin()
}
