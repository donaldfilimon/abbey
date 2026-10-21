import {expect,test} from "bun:test";
import {renderToStaticMarkup} from "react-dom/server";
import {MessageContent,parseMessage,copyCode} from "../src/conversations/MessageContent";
test("parses Unicode paragraphs, lists, headings and exact fenced code",()=>{
  expect(parseMessage("# Résumé\n\n- 一\n- two\n\n```rust\nlet x = 1;\n```" )).toEqual([{kind:"heading",level:1,text:"Résumé"},{kind:"list",ordered:false,items:["一","two"]},{kind:"code",language:"rust",text:"let x = 1;\n",closed:true}]);
  expect(parseMessage("```\nunfinished").at(-1)).toMatchObject({kind:"code",closed:false,text:"unfinished"});
});
test("renders markup safely, no remote assets, active HTML, dangerous URL or dead link",()=>{
  const html=renderToStaticMarkup(<MessageContent text={'<img src="https://evil.test/x">\n![remote](https://evil.test/image)\n[bad](javascript:alert(1))\n[data](data:text/html,x)\n[asset](asset://x)\n[safe](https://example.com)\n**bold** and `code`'} complete={false}/>);
  expect(html).not.toContain("<img");expect(html).not.toContain("<a ");expect(html).not.toContain("dangerouslySetInnerHTML");expect(html).toContain("&lt;img");expect(html).toContain("<strong>bold</strong>");expect(html).toContain("<code>code</code>");expect(html).toContain("Incomplete message");expect(html).toContain("Links are shown as text");
});
test("code renders selectable preformatted text and unavailable clipboard is explicit",()=>{
  const html=renderToStaticMarkup(<MessageContent text={'```sh\necho "雪"\n```'} complete/>);
  expect(html).toContain("<pre");expect(html).toContain("雪");expect(html).toContain("Clipboard unavailable");expect(html).toContain("disabled");
});
test("copy reports success, missing permission and unavailable clipboard without throwing",async()=>{
  let saved="";expect(await copyCode("exact\n",async text=>{saved=text;})).toBe("copied");expect(saved).toBe("exact\n");expect(await copyCode("x",async()=>{throw Error("permission denied");})).toBe("failed");expect(await copyCode("x")).toBe("unavailable");
});
test("fences retain exact final newline, support tilde delimiters and do not swallow shorter closers",()=>{
  expect(parseMessage("```\na\n")[0]).toMatchObject({text:"a\n",closed:false});
  expect(parseMessage("~~~~txt\na\n~~~\nb\n~~~~")[0]).toMatchObject({language:"txt",text:"a\n~~~\nb\n",closed:true});
  expect(parseMessage("1. first\n2. second\n- third")).toEqual([{kind:"list",ordered:true,items:["first","second"]},{kind:"list",ordered:false,items:["third"]}]);
});
test("4MiB newline or inline-heavy text falls back losslessly to bounded plain text",()=>{
  for(const text of ["a\n".repeat(2*1024*1024),"**x** ".repeat(700000)]){
    const blocks=parseMessage(text);expect(blocks).toHaveLength(1);expect(blocks[0]).toEqual({kind:"plain",text});
    const html=renderToStaticMarkup(<MessageContent text={text} complete/>);expect(html.match(/<pre/g)).toHaveLength(1);expect(html).not.toContain("<strong>");
  }
});
