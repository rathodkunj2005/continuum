//  ConnectionStatusService.swift
//
//  Polls the Mac's /v1/status on a low duty cycle and exposes a snapshot
//  the SwiftUI Status tab subscribes to. The cadence backs off when the
//  Mac is unreachable so a sleeping MacBook doesn't burn the iPhone's
//  radio.

import Foundation

public struct ConnectionSnapshot: Equatable, Sendable {
    public enum Reachability: String, Sendable {
        case unknown
        case reachable
        case unreachable
    }

    public var reachability: Reachability
    public var status: StatusResponse?
    public var lastCheckedAtMs: Int64?
    public var lastErrorMessage: String?

    public static let unknown = ConnectionSnapshot(
        reachability: .unknown,
        status: nil,
        lastCheckedAtMs: nil,
        lastErrorMessage: nil
    )
}

public protocol ConnectionStatusSink: AnyObject, Sendable {
    func didUpdate(snapshot: ConnectionSnapshot)
}

public actor ConnectionStatusService {
    public struct Cadence: Sendable {
        public let foregroundIntervalMs: UInt64
        public let backoffIntervalMs: UInt64

        public static let `default` = Cadence(
            foregroundIntervalMs: 5_000,
            backoffIntervalMs: 30_000
        )

        public init(foregroundIntervalMs: UInt64, backoffIntervalMs: UInt64) {
            self.foregroundIntervalMs = foregroundIntervalMs
            self.backoffIntervalMs = backoffIntervalMs
        }
    }

    private let client: CompanionClient
    private let cadence: Cadence
    private let now: @Sendable () -> Int64
    private weak var sink: ConnectionStatusSink?
    private var pollTask: Task<Void, Never>?
    private var snapshot: ConnectionSnapshot = .unknown

    public init(
        client: CompanionClient,
        cadence: Cadence = .default,
        sink: ConnectionStatusSink? = nil,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
    ) {
        self.client = client
        self.cadence = cadence
        self.sink = sink
        self.now = now
    }

    public func currentSnapshot() -> ConnectionSnapshot { snapshot }

    public func setSink(_ sink: ConnectionStatusSink?) {
        self.sink = sink
    }

    public func start() {
        guard pollTask == nil else { return }
        pollTask = Task { [weak self] in
            await self?.pollLoop()
        }
    }

    public func stop() {
        pollTask?.cancel()
        pollTask = nil
    }

    /// Perform a single status check now (useful at app foreground).
    @discardableResult
    public func refreshOnce() async -> ConnectionSnapshot {
        await runOneTick()
        return snapshot
    }

    private func pollLoop() async {
        while !Task.isCancelled {
            await runOneTick()
            let delayMs = snapshot.reachability == .reachable
                ? cadence.foregroundIntervalMs
                : cadence.backoffIntervalMs
            do {
                try await Task.sleep(nanoseconds: delayMs * 1_000_000)
            } catch {
                break
            }
        }
    }

    private func runOneTick() async {
        let started = now()
        do {
            let status = try await client.status()
            snapshot = ConnectionSnapshot(
                reachability: .reachable,
                status: status,
                lastCheckedAtMs: started,
                lastErrorMessage: nil
            )
        } catch {
            snapshot = ConnectionSnapshot(
                reachability: .unreachable,
                status: snapshot.status,
                lastCheckedAtMs: started,
                lastErrorMessage: error.localizedDescription
            )
        }
        sink?.didUpdate(snapshot: snapshot)
    }
}
