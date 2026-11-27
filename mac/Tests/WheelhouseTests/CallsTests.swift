import Foundation
import Testing
@testable import Wheelhouse

// A call's proposals in words, and what Edit may change (Calls.swift, as
// web/src/calls.ts and web/test/calls.test.mts).

private func p(_ o: [String: Any] = [:]) -> Proposal {
    var base: [String: Any] = ["n": 1, "seq": 4, "at": "t", "call": 3, "kind": "decide", "options": [String](), "quote": "q", "quote_found": true, "status": "open"]
    base.merge(o) { _, new in new }
    return Proposal(j: JSON(any: base))
}

private func call(_ listening: [String: Any], _ n: Int = 0) -> CallView {
    CallView(j: JSON(any: ["call": 3, "title": "Sync", "at": "t", "listening": listening, "proposals": (0 ..< n).map { i in ["n": i + 1, "call": 3, "kind": "decide", "quote": "q", "status": "open"] }]))
}

@Test func whatApplyingAProposalWouldDo() {
    #expect(Calls.whatItDoes(p(["docket": "D-41", "subject": "Retries", "choice": "5xx only"])) == "Decide on D-41: Retries: 5xx only")
    #expect(Calls.whatItDoes(p(["docket": "D-41", "choice": "5xx only"])) == "Decide on D-41: 5xx only")
    #expect(Calls.whatItDoes(p(["kind": "ask", "docket": "D-41", "question": "Do 429s count?\nAnd 408s?"])) == "Ask the agents on D-41: Do 429s count?")
    #expect(Calls.whatItDoes(p(["kind": "task", "title": "Write the runbook"])) == "Open a docket: Write the runbook")
    #expect(Calls.whatItDoes(p(["kind": "intent", "docket": "D-41", "subject": "Log each attempt"])) == "Revise the intent on D-41: Log each attempt")
    #expect(Calls.whatItDoes(p(["kind": "intent", "docket": "D-41"])) == "Revise the intent on D-41")
    #expect(Calls.whatItDoes(p(["kind": "instruct", "docket": "D-41", "text": "Log each retry.\nWith its number."])) == "Tell the agents on D-41: Log each retry.")
    #expect(Calls.whatItDoes(p(["choice": "x"])) == "Decide: x", "no docket yet: no “on”")
}

@Test func kindsInWords() {
    #expect(Calls.kindWord(.intent) == "Intent")
    #expect(Calls.kindWord(.decide) == "Decision")
    #expect(Calls.kindWord(.instruct) == "Instruction")
    #expect(Calls.kindWord(.ask) == "Question")
    #expect(Calls.kindWord(.task) == "Task")
    #expect(p(["kind": "ask"]).kind == .ask)
    #expect(p(["kind": "intent"]).kind == .intent)
    #expect(p(["kind": "instruct"]).kind == .instruct)
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
    #expect(Calls.cantApply(p(["kind": "instruct", "docket": "D-1", "text": "  "])) == "Nothing to do")
    #expect(Calls.cantApply(p(["kind": "instruct", "docket": "D-1", "text": "Log it"])) == nil)
    #expect(Calls.cantApply(p(["kind": "intent", "docket": "D-1", "intent": "x", "base": 2]), versions: 2) == nil)
    #expect(Calls.cantApply(p(["kind": "intent", "docket": "D-1", "intent": "x", "base": 2]), versions: 3) == "The intent changed since the call was read: Listen again, or Edit", "read against an older intent")
    #expect(Calls.cantApply(p(["kind": "intent", "docket": "D-1", "intent": "x", "base": 2])) == nil, "without the versions now, nothing to tell")
    #expect(Calls.cantApply(p(["kind": "intent", "docket": "D-1", "intent": "x"]), versions: 3) == nil, "no base: nothing to tell")
    #expect(Calls.cantApply(p(["kind": "intent", "docket": "D-1", "intent": " "]), versions: 1) == "No intent")
    #expect(Calls.cantApply(p(["kind": "intent", "intent": "x"])) == "Pick the docket it's for: Edit")
}

@Test func editShowsTheFieldsOfItsKind() {
    #expect(Calls.editable(.intent) == ["subject", "intent"], "never another docket's")
    #expect(Calls.editable(.decide) == ["docket", "subject", "choice"])
    #expect(Calls.editable(.instruct) == ["docket", "text"])
    #expect(Calls.editable(.ask) == ["docket", "question", "options"])
    #expect(Calls.editable(.task) == ["title", "intent"])
}

@Test func optionsAsTyped() {
    #expect(Calls.parseOptions("Yes\n No | Maybe\r\n\n") == ["Yes", "No | Maybe"], "one per line: an option may hold a |")
    #expect(Calls.parseOptions("a\r\nb") == ["a", "b"])
    #expect(Calls.parseOptions("") == [])
    #expect(Calls.parseOptions("1\n2\n3\n4\n5\n6\n7\n8") == ["1", "2", "3", "4", "5", "6"], "six at most")
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
    #expect(out == JSON(any: ["docket": NSNull(), "question": "Which one?", "options": ["A | B", "C"]]))
    let task = JSON(any: Calls.edit(.task, ["title": "Runbook", "intent": ""]))
    #expect(task == JSON(any: ["title": "Runbook", "intent": NSNull()]))
    let tell = p(["kind": "instruct", "docket": "D-4", "text": "Log each retry."])
    #expect(Calls.fields(tell)["text"] == "Log each retry.")
    #expect(JSON(any: Calls.edit(.instruct, Calls.fields(tell))) == JSON(any: ["docket": "D-4", "text": "Log each retry."]))
    let revise = p(["kind": "intent", "docket": "D-4", "subject": "Log it", "intent": "Retry.\n\n- [ ] Log it"])
    #expect(JSON(any: Calls.edit(.intent, Calls.fields(revise), version: 3)) == JSON(any: ["subject": "Log it", "intent": "Retry.\n\n- [ ] Log it", "version": 3]), "the intent, and the version it was edited against")
    #expect(JSON(any: Calls.edit(.instruct, Calls.fields(tell), version: 3)) == JSON(any: ["docket": "D-4", "text": "Log each retry."]), "only an intent change sends a version")
    var cleared = Calls.fields(tell)
    cleared["docket"] = ""
    #expect(JSON(any: Calls.edit(.instruct, cleared))["docket"] == .null, "a docket cleared goes as null: the hub refuses it, never takes it as this one")
}

@Test func theListenersWordsLoadNoImage() {
    let text = "![chart](https://example.com/a.png)"
    #expect(MD.blocks(text, size: 13) == [.image(URL(string: "https://example.com/a.png")!, alt: "chart")], "an image of yours shows")
    let quiet = MD.blocks(text, size: 13, images: false)
    #expect(quiet.count == 1)
    if case let .paragraph(a)? = quiet.first { #expect(String(a.characters) == "chart") } else { Issue.record("not its words: \(quiet)") }
    let nested = MD.blocks("- ![x](https://example.com/b.png)\n\n> ![y](https://example.com/c.png)", size: 13, images: false)
    #expect(!"\(nested)".contains("image("), "nor in a list or a quote")
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
    #expect(Calls.tally(call(["state": "done"]).proposals) == "no proposals")
    #expect(Calls.tally([p(["status": "open"]), p(["status": "applied"]), p(["status": "applied"])]) == "1 open · 2 applied")
    #expect(Calls.tally([p(["status": "applied"])]) == "1 applied")
    #expect(Calls.tally([p(["status": "ignored"])]) == "all ignored")
}

@Test func aCallViewsFields() {
    let c = call(["state": "done"], 2)
    #expect(c.call == 3)
    #expect(c.id == 3)
    #expect(c.title == "Sync")
    #expect(c.at == "t")
    #expect(c.proposals.map(\.call) == [3, 3])
    #expect(p(["text": "Log it", "base": 2]).text == "Log it")
    #expect(p(["base": 2]).base == 2)
    #expect(p().base == nil)
}

@Test func theCallsSectionAndAddingOne() {
    #expect(Calls.sectionMeta(own: true, count: 1) == "from the call's transcript")
    #expect(Calls.sectionMeta(own: false, count: 1) == "1 call")
    #expect(Calls.sectionMeta(own: false, count: 3) == "3 calls")
    #expect(Calls.sectionMeta(own: false, count: 0) == "what was said on calls about this docket")
    #expect(Calls.readsAgainst("D-41") == "The Listener reads it against D-41: its intent, decisions and agents, and proposes. Nothing changes until you apply it.")
    #expect(Calls.readsAgainst(nil) == "The Listener reads it against your open dockets and proposes. Nothing changes until you apply it.")
}

@Test func aDocketACallCanBeKeptOn() {
    let d = { (o: [String: Any]) in Docket(j: JSON(any: ["id": "D-41", "status": "in_progress", "protocol": "agent", "agents": [Any]()].merging(o) { _, new in new })) }
    #expect(d([:]).callable)
    #expect(d(["status": "todo"]).callable)
    #expect(!d(["status": "done"]).callable, "not a closed one")
    #expect(!d(["status": "dropped"]).callable)
    #expect(!d(["protocol": "call"]).callable, "not a call's own docket")
    #expect(!d(["id": "ACME-1"]).callable, "not a workspace's docket")
    #expect(!d(["shared_as": "ACME-7"]).callable, "not one shared with the team: it continues there")
    #expect(d(["shared_as": NSNull()]).callable)
    #expect(d(["shared_as": ""]).callable, "a blank shared_as is none")
}

@Test func anIntentChangeFolded() {
    let from = (1 ... 10).map { "line \($0)" }.joined(separator: "\n")
    let to = from.replacingOccurrences(of: "line 5", with: "line five")
    let folded = foldDiff(lineDiff(from, to), context: 2)
    #expect(folded.first == .skip(2))
    #expect(folded.last == .skip(3))
    #expect(folded.filter { if case .line = $0 { true } else { false } }.count == 6, "two around the change, and the change itself")
    #expect(folded.contains(.line(DiffLine(op: .del, text: "line 5"))))
    #expect(folded.contains(.line(DiffLine(op: .add, text: "line five"))))
    #expect(foldDiff(lineDiff("a\nb", "a\nb")) == [.skip(2)], "nothing changed: all folded")
    #expect(foldDiff(lineDiff("", "a")) == [.line(DiffLine(op: .add, text: "a"))])
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
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-41", "did": "intent"])) == "D-41's intent revised: a new version")
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-41", "did": "instructed", "sent": [["to": "builder"]]])) == "Sent to the agents on D-41")
    #expect(Calls.appliedToast(JSON(any: ["docket": "D-41", "did": "instructed", "told": ["builder"]])) == "Applied to D-41, and told builder")
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

@Test func aCallDocketTakesNoAgentAndAPageHasItsCalls() {
    let d = Docket(j: JSON(any: ["id": "D-5", "status": "todo", "protocol": "call", "agents": [Any]()]))
    #expect(d.isCall)
    #expect(!d.canStartAgent)
    #expect(Docket(j: JSON(any: ["id": "D-6", "status": "todo", "protocol": "none", "agents": [Any]()])).canStartAgent)
    let page = DocketPage(JSON(any: ["docket": ["id": "D-5", "protocol": "call"], "entries": [Any](), "calls": [
        ["call": 7, "title": "Later", "at": "t2", "listening": ["state": "started"], "proposals": [Any]()],
        ["call": 2, "title": "Sync", "at": "t1", "listening": ["state": "done"], "proposals": [["n": 1, "call": 2, "kind": "ask", "question": "Q?", "quote": "q", "quote_found": false, "status": "open"]]],
    ]]))
    #expect(page.calls?.map(\.call) == [7, 2], "newest first, as the hub sends them")
    #expect(page.calls?.first?.reading == true)
    #expect(page.calls?.last?.proposals.count == 1)
    #expect(page.calls?.last?.proposals.first?.quoteFound == false)
    #expect(DocketPage(JSON(any: ["docket": ["id": "D-6"], "entries": [Any]()])).calls == nil, "no call: no section")
    #expect(DocketPage(JSON(any: ["docket": ["id": "D-6"], "entries": [Any](), "calls": [Any]()])).calls?.isEmpty == true)
    #expect(DocketPage(JSON(any: ["docket": ["id": "D-6"], "entries": [Any](), "call": ["listening": ["state": "done"], "proposals": [Any]()]])).calls == nil, "the old page's one call isn't read")
}
