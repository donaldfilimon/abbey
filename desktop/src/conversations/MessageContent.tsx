import {Fragment, useEffect, useState, type ReactNode} from "react";

type Block = {kind:"plain"; text:string} | {kind:"paragraph"; text:string} | {kind:"heading"; level:number; text:string} | {kind:"list"; ordered:boolean; items:string[]} | {kind:"code"; language:string; text:string; closed:boolean};
// A small bounded, non-HTML Markdown subset. Unrecognized syntax stays text.
export function parseMessage(text:string):Block[] {
  // Bound work before splitting or tokenizing. Large/complex replies stay exact
  // selectable text in one node rather than allocating a node per line/token.
  let complexity=0;
  if(text.length>65536)return [{kind:"plain",text}];
  for(const character of text)if(character==="\n" || character==="*" || character==="`") {
    if(++complexity>256)return [{kind:"plain",text}];
  }
  const lines=text.split("\n"), blocks:Block[]=[];
  for(let i=0;i<lines.length;) {
    const line=lines[i]!;
    if(!line.trim()){i++;continue;}
    const fence=/^ {0,3}(`{3,}|~{3,})([^\s`~]*)\s*$/.exec(line);
    if(fence) {
      const delimiter=fence[1]!, start=++i, close=new RegExp(`^ {0,3}${delimiter[0]}{${delimiter.length},}\\s*$`);
      while(i<lines.length && !close.test(lines[i]!)) i++;
      const closed=i<lines.length;
      const body=lines.slice(start,i).join("\n")+(i>start && closed ? "\n" : "");
      blocks.push({kind:"code",language:fence[2]!.slice(0,32),text:body,closed});
      if(closed)i++;
      continue;
    }
    const heading=/^(#{1,6})\s+(.+)$/.exec(line);
    if(heading){blocks.push({kind:"heading",level:heading[1]!.length,text:heading[2]!});i++;continue;}
    const list=/^\s*(?:([-+*])|\d+\.)\s+(.+)$/.exec(line);
    if(list){const ordered=!list[1],items:string[]=[];while(i<lines.length){const item=/^\s*(?:([-+*])|\d+\.)\s+(.+)$/.exec(lines[i]!);if(!item || (!item[1])!==ordered)break;items.push(item[2]!);i++;}blocks.push({kind:"list",ordered,items});continue;}
    // Preserve hard line breaks instead of silently joining streamed prose.
    blocks.push({kind:"paragraph",text:line});i++;
  }
  return blocks;
}
function inline(text:string):ReactNode[] {
  const tokens=text.split(/(`[^`\n]+`|\*\*[^*\n]+\*\*)/g);
  return tokens.map((token,index)=>token.startsWith("`") && token.endsWith("`") ? <code key={index}>{token.slice(1,-1)}</code> : token.startsWith("**") && token.endsWith("**") ? <strong key={index}>{token.slice(2,-2)}</strong> : <Fragment key={index}>{token}</Fragment>);
}
export async function copyCode(text:string,write?: (text:string)=>Promise<void>):Promise<"copied" | "failed" | "unavailable"> {
  if(!write)return "unavailable";
  try{await write(text);return "copied";}catch{return "failed";}
}
function CodeBlock({block}:{block:Extract<Block,{kind:"code"}>}) {
  const [status,setStatus]=useState<"ready" | "copying" | "copied" | "failed" | "unavailable">("ready");
  useEffect(()=>setStatus("ready"),[block.text]);
  const available=typeof navigator!=="undefined" && typeof navigator.clipboard?.writeText==="function";
  const copy=async()=>{
    if(status==="copying")return;
    setStatus("copying");
    setStatus(await copyCode(block.text,available ? text=>navigator.clipboard.writeText(text) : undefined));
  };
  return <div className="message-code"><div className="message-code-toolbar"><span>{block.language || "Code"}{!block.closed ? " · incomplete fence" : ""}</span><button type="button" disabled={!available || status==="copying"} onClick={()=>void copy()}>{!available || status==="unavailable" ? "Clipboard unavailable" : status==="copying" ? "Copying…" : status==="copied" ? "Copied" : "Copy code"}</button></div><pre tabIndex={0} aria-label={block.language ? `${block.language} code` : "Code"}><code>{block.text}</code></pre>{status==="failed" && <p role="status" className="message-copy-status">Copy failed. Select the code and copy it manually.</p>}{status==="copied" && <span role="status" className="message-copy-status">Code copied.</span>}</div>;
}
export function MessageContent({text,complete}:{text:string;complete:boolean}) {
  const blocks=parseMessage(text);
  return <div className="message-content">{blocks.map((block,index)=>{
    if(block.kind==="plain")return <pre className="message-plain" key={index}>{block.text}</pre>;
    if(block.kind==="code")return <CodeBlock key={index} block={block}/>;
    if(block.kind==="list")return block.ordered ? <ol key={index}>{block.items.map((item,i)=><li key={i}>{inline(item)}</li>)}</ol> : <ul key={index}>{block.items.map((item,i)=><li key={i}>{inline(item)}</li>)}</ul>;
    if(block.kind==="heading")return <p className={`message-heading message-heading-${block.level}`} key={index}>{inline(block.text)}</p>;
    return <p key={index}>{inline(block.text)}</p>;
  })}{(text.includes("[") && text.includes("](")) && <p className="message-content-note">Links are shown as text; external opening is unavailable here.</p>}{!complete && <p className="message-content-note" role="status">Incomplete message. More text may be available.</p>}</div>;
}
