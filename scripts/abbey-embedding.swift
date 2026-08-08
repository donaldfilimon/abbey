#!/usr/bin/swift

import Foundation
import NaturalLanguage

struct Request: Decodable {
    let language: String
    let input: [String]
}

struct Item: Encodable {
    let index: Int
    let embedding: [Double]
}

struct Response: Encodable {
    let data: [Item]
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(2)
}

let requestData = FileHandle.standardInput.readDataToEndOfFile()
let request: Request
do {
    request = try JSONDecoder().decode(Request.self, from: requestData)
} catch {
    fail("invalid embedding request: \(error)")
}

let language = NLLanguage(rawValue: request.language)
guard let model = NLEmbedding.sentenceEmbedding(for: language) else {
    fail("NaturalLanguage has no sentence embedding for language \(request.language)")
}

var items: [Item] = []
for (index, text) in request.input.enumerated() {
    guard let vector = model.vector(for: text) else {
        fail("NaturalLanguage could not embed input at index \(index)")
    }
    items.append(Item(index: index, embedding: vector))
}

do {
    let encoded = try JSONEncoder().encode(Response(data: items))
    FileHandle.standardOutput.write(encoded)
} catch {
    fail("could not encode embedding response: \(error)")
}
