//  Assertions.swift — tiny assertion helpers used by FNDRKitCheck.
//
//  XCTest isn't available without full Xcode (Command Line Tools alone
//  doesn't ship it), so the slice-2 tests live as plain functions inside
//  this executable target. The same logic is also expressed as XCTest
//  cases in `Tests/FNDRKitTests/` for Xcode users; both call into the same
//  public surface of FNDRKit.

import Foundation

public struct AssertionFailure: Error, CustomStringConvertible {
    public let message: String
    public let file: StaticString
    public let line: UInt
    public var description: String { "\(file):\(line) — \(message)" }
}

public func expect(
    _ condition: @autoclosure () throws -> Bool,
    _ message: @autoclosure () -> String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    if try !condition() {
        throw AssertionFailure(message: message().isEmpty ? "expectation failed" : message(), file: file, line: line)
    }
}

public func expectEqual<T: Equatable>(
    _ a: @autoclosure () throws -> T,
    _ b: @autoclosure () throws -> T,
    _ message: @autoclosure () -> String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    let av = try a()
    let bv = try b()
    if av != bv {
        let prefix = message().isEmpty ? "expected equal" : message()
        throw AssertionFailure(message: "\(prefix): \(av) != \(bv)", file: file, line: line)
    }
}

public func expectNil<T>(
    _ value: @autoclosure () throws -> T?,
    _ message: @autoclosure () -> String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    if let v = try value() {
        let prefix = message().isEmpty ? "expected nil" : message()
        throw AssertionFailure(message: "\(prefix), got \(v)", file: file, line: line)
    }
}

@discardableResult
public func expectNotNil<T>(
    _ value: @autoclosure () throws -> T?,
    _ message: @autoclosure () -> String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws -> T {
    if let v = try value() { return v }
    let prefix = message().isEmpty ? "expected non-nil" : message()
    throw AssertionFailure(message: prefix, file: file, line: line)
}

public func expectThrows<E: Error>(
    _ block: () throws -> Void,
    _ errorType: E.Type,
    _ message: @autoclosure () -> String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    do {
        try block()
    } catch is E {
        return
    } catch {
        let prefix = message().isEmpty ? "wrong error type" : message()
        throw AssertionFailure(message: "\(prefix): expected \(E.self), got \(type(of: error))", file: file, line: line)
    }
    let prefix = message().isEmpty ? "no error thrown" : message()
    throw AssertionFailure(message: "\(prefix): expected \(E.self)", file: file, line: line)
}

public func expectThrowsMatching<E: Error & Equatable>(
    _ block: () async throws -> Void,
    _ expected: E,
    _ message: @autoclosure () -> String = "",
    file: StaticString = #file,
    line: UInt = #line
) async throws {
    do {
        try await block()
    } catch let err as E where err == expected {
        return
    } catch {
        let prefix = message().isEmpty ? "wrong error" : message()
        throw AssertionFailure(message: "\(prefix): expected \(expected), got \(error)", file: file, line: line)
    }
    let prefix = message().isEmpty ? "no error thrown" : message()
    throw AssertionFailure(message: "\(prefix): expected \(expected)", file: file, line: line)
}

// MARK: - Test runner

public struct TestCase {
    public let name: String
    public let run: () async throws -> Void

    public init(_ name: String, _ run: @escaping () async throws -> Void) {
        self.name = name
        self.run = run
    }
}

public struct TestSuite {
    public let name: String
    public let cases: [TestCase]

    public init(_ name: String, _ cases: [TestCase]) {
        self.name = name
        self.cases = cases
    }
}

public func runSuites(_ suites: [TestSuite]) async -> Int32 {
    var passed = 0
    var failed = 0
    var failures: [String] = []
    let startedAt = Date()

    for suite in suites {
        print("▸ \(suite.name)")
        for testCase in suite.cases {
            do {
                try await testCase.run()
                passed += 1
                print("  ✓ \(testCase.name)")
            } catch let failure as AssertionFailure {
                failed += 1
                let line = "  ✗ \(testCase.name)\n      \(failure.description)"
                print(line)
                failures.append(line)
            } catch {
                failed += 1
                let line = "  ✗ \(testCase.name)\n      \(error)"
                print(line)
                failures.append(line)
            }
        }
    }

    let elapsed = String(format: "%.2f", Date().timeIntervalSince(startedAt))
    print("\n— Summary —")
    print("passed: \(passed)   failed: \(failed)   in \(elapsed)s")

    if failed == 0 { return 0 }
    print("\nFailures:")
    for f in failures { print(f) }
    return 1
}
