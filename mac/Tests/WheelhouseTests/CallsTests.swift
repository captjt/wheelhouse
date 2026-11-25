import Foundation
import Testing
@testable import Wheelhouse

// A call's proposals in words, and what Edit may change (Calls.swift, as
// web/src/calls.ts and web/test/calls.test.mts).

private func p(_ o: [String: Any] = [:]) -> Proposal {
    var base: [String: Any] = ["n": 1, "seq": 4, "at": "t", "kind": "decide", "options": [String](), "quote": "q", "quote_found": true, "status": "open"]
    base.merge(o) { _, new in new }
    return Proposal(j: JSON(any: base))
}

private func call(_ listening: [String: Any], _ n: Int = 0) -> CallView {
    CallView(j: JSON(any: ["listening": listening, "proposals": (0 ..< n).map { i in ["n": i + 1, "kind": "decide", "quote": "q", "status": "open"] }]))
}

@Test func whatApplyingAProposalWouldDo() {
    #expect(Calls.whatItDoes(p(["docket": "D-41", "subject": "Retries", "choice": "5xx only"])) == "Decide on D-41: Retries: 5xx only")
    #expect(Calls.whatItDoes(p(["docket": "D-41", "choice": "5xx only"])) == "Decide on D-41: 5xx only")
    #expect(Calls.whatItDoes(p(["kind": "ask", "docket": "D-41", "question": "Do 429s count?\nAnd 408s?"])) == "Ask the agents on D-41: Do 429s count?")
    #expect(Calls.whatItDoes(p(["kind": "task", "title": "Write the runbook"])) == "Open a docket: Write the runbook")
    #expect(Calls.whatItDoes(p(["choice": "x"])) == "Decide: x", "no docket yet: no “on”")
}

@Test func kindsInWords() {
    #expect(Calls.kindWord(.decide) == "Decision")
    #expect(Calls.kindWord(.ask) == "Question")
    #expect(Calls.kindWord(.task) == "Task")
    #expect(p(["kind": "ask"]).kind == .ask)
    #expect(p(["kind": "something new"]).kind == .task, "an unknown kind reads as the web app's last word")
}

@Test func applyWaitsForADocketAndTheWordsItsKindNeeds() {
    #expect(Calls.cantApply(p(["choice": "x"])) == "Pick the docket it's for: Edit")
    #expect(Calls.cantApply(p(["docket": "  ", "choice": "x"])) == "Pick the docket it's for: Edit", "a blank docket is none")
    #expect(Calls.cantApply(p(["docket": "D-1", "choice": " "])) == "Nothing decided")
    #expect(Calls.cantApply(p(["kind": "ask", "docket": "D-1"])) == "No question")
    #expect(Calls.cantApply(p(["kind": "task", "title": ""])) == "No title")
    #expect(Calls.cantApply(p(["kind": "task", "title": "T"])) == nil, "a task needs no docket")
    #expect(Calls.cantApply(p(["docket": "D-1", "choice": "x"])) == nil)
}

@Test func editShowsTheFieldsOfItsKind() {
    #expect(Calls.editable(.decide) == ["docket", "subject", "choice"])
    #expect(Calls.editable(.ask) == ["docket", "question", "options"])
    #expect(Calls.editable(.task) == ["title", "intent"])
}

@Test func optionsAsTyped() {
    #expect(Calls.parseOptions("Yes\n No | Maybe\n\n") == ["Yes", "No", "Maybe"])
    #expect(Calls.parseOptions("a\r\nb") == ["a", "b"])
    #expect(Calls.parseOptions("") == [])
    #expect(Calls.parseOptions("1|2|3|4|5|6|7|8") == ["1", "2", "3", "4", "5", "6"], "six at most")
}

@Test func editSendsItsKindsFieldsBlankAsNull() {
    let q = p(["kind": "ask", "docket": "D-4", "question": "Which?", "options": ["A", "B"]])
    var f = Calls.fields(q)
    #expect(f["options"] == "A\nB")
    #expect(f["question"] == "Which?")
    f["question"] = "  Which one?  "
    f["options"] = "A | B\nC"
    f["docket"] = ""
    f["title"] = "not this kind's"
    let out = JSON(any: Calls.edit(.ask, f))
    #expect(out == JSON(any: ["docket": NSNull(), "question": "Which one?", "options": ["A", "B", "C"]]))
    let task = JSON(any: Calls.edit(.task, ["title": "Runbook", "intent": ""]))
    #expect(task == JSON(any: ["title": "Runbook", "intent": NSNull()]))
}

@Test func howListeningWentInWords() {
    #expect(Calls.listeningWords(call(["state": "started"]))?.tone == .busy)
    #expect(Calls.listeningWords(call(["state": "failed", "why": "it timed out"]))?.text.contains("it timed out") == true)
    #expect(Calls.listeningWords(call(["state": "failed"]))?.text == "The Listener couldn't read the call: it failed")
    #expect(Calls.listeningWords(call(["state": "stopped"]))?.tone == .error)
    #expect(Calls.listeningWords(call(["state": "done"]))?.text.contains("nothing") == true)
    #expect(Calls.listeningWords(call(["state": "done", "left_out": ["x", "y"]], 2))?.text.hasPrefix("2 more") == true)
    #expect(Calls.listeningWords(call(["state": "done"], 3)) == nil)
    #expect(Calls.listeningWords(call(["state": NSNull()])) == nil)
    #expect(call(["state": "started"]).reading)
    #expect(!call(["state": "done"]).reading)
}

@Test func aProposalsOutcome() {
    let applied = p(["status": "applied", "outcome": ["seq": 9, "at": "2026-11-25T10:00:00Z", "applied": ["docket": "D-41", "did": "decided"], "edited": ["choice": "x"]]])
    #expect(!applied.isOpen)
    #expect(applied.appliedTo == "D-41")
    #expect(applied.edited)
    #expect(applied.answeredAt == "2026-11-25T10:00:00Z")
    let ignored = p(["status": "ignored", "outcome": ["seq": 9, "at": "t", "applied": NSNull(), "edited": NSNull()]])
    #expect(ignored.appliedTo == nil)
    #expect(!ignored.edited)
    #expect(p().isOpen)
}

@Test func whatApplyingDidForAToast() {
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-9", "did": "opened"])) == "D-9 opened from the call")
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-41", "did": "decided", "told": ["builder"]])) == "Applied to D-41, and told builder")
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-41", "did": "decided", "told": ["a", "b"]])) == "Applied to D-41, and told 2 agents")
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-41", "did": "asked"])) == "Applied to D-41")
}

@Test func aCallsStepsOnTheRecord() {
    #expect(Calls.proposedBody("Decide on D-41: 5xx", quote: "let's do\n5xx only") == "Decide on D-41: 5xx\n\n> let's do 5xx only")
    #expect(Calls.answered(JSON(any: ["proposal": 3, "choice": "Apply", "applied": ["docket": "D-41"], "edited": ["choice": "x"]])) == "You applied #3 to D-41, edited")
    #expect(Calls.answered(JSON(any: ["proposal": 3, "choice": "Apply", "applied": ["docket": "D-41"], "edited": NSNull()])) == "You applied #3 to D-41")
    #expect(Calls.answered(JSON(any: ["proposal": 4, "choice": "Ignore"])) == "You ignored #4")
}

@Test func aTranscriptsSizeAndFile() {
    #expect(!Calls.tooLong(String(repeating: "x", count: Calls.maxTranscript)))
    #expect(Calls.tooLong(String(repeating: "x", count: Calls.maxTranscript + 1)))
    #expect(!Calls.tooLong("  " + String(repeating: "x", count: Calls.maxTranscript) + "\n\n"), "the blanks at either end don't count, as the hub trims them")
    #expect(Calls.tooLong(String(repeating: "é", count: Calls.maxTranscript / 2 + 1)), "bytes, not characters")
    #expect(!Calls.fileTooBig(400_000))
    #expect(Calls.fileTooBig(400_001))
    #expect(Calls.fileText(Data("\u{FEFF}Sam: hi".utf8)) == "Sam: hi")
    #expect(Calls.fileText(Data([0xFF, 0xFE, 0x00, 0x41])) == nil, "not UTF-8")
    #expect(Calls.fileText(Data("a\0b".utf8)) == nil, "a NUL: not text")
    #expect(Calls.fileTitle("Webhooks sync.vtt") == "Webhooks sync")
    #expect(Calls.fileTitle("notes.2026.md") == "notes.2026")
    #expect(Calls.fileTitle("README") == "README")
    #expect(Calls.fileTitle(".hidden") == ".hidden")
    #expect(Calls.fileTitle("odd.tx t") == "odd.tx t")
}

@Test func aCallDocketTakesNoAgentAndItsPageHasItsProposals() {
    let d = Docket(j: JSON(any: ["id": "D-5", "status": "todo", "protocol": "call", "agents": [Any]()]))
    #expect(d.isCall)
    #expect(!d.canStartAgent)
    #expect(Docket(j: JSON(any: ["id": "D-6", "status": "todo", "protocol": "none", "agents": [Any]()])).canStartAgent)
    let page = DocketPage(JSON(any: ["docket": ["id": "D-5", "protocol": "call"], "entries": [Any](), "call": ["listening": ["state": "done"], "proposals": [["n": 1, "kind": "ask", "question": "Q?", "quote": "q", "quote_found": false, "status": "open"]]]]))
    #expect(page.call?.proposals.count == 1)
    #expect(page.call?.proposals.first?.quoteFound == false)
    #expect(DocketPage(JSON(any: ["docket": ["id": "D-6"], "entries": [Any]()])).call == nil)
}
