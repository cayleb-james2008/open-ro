// node stub-model.mjs <port> <replies.json>   — OpenAI-compatible chat/completions stub.
// replies.json = ["{\"done\":true,\"summary\":\"ok\"}", ...] ; each call returns the next, the last repeats.
// Every request body is appended to <replies.json>.requests.jsonl so you can see what the app sent.
import http from 'node:http'
import fs from 'node:fs'
const [port, file] = [Number(process.argv[2]), process.argv[3]]
let replies = JSON.parse(fs.readFileSync(file, 'utf8')), i = 0
http.createServer((req, res) => {
  let body = ''
  req.on('data', d => body += d)
  req.on('end', () => {
    fs.appendFileSync(file + '.requests.jsonl', JSON.stringify({ url: req.url, auth: req.headers.authorization ? 'present' : 'none', session: req.headers['x-opencode-session'] || null, body: (() => { try { return JSON.parse(body) } catch { return body.slice(0, 500) } })() }) + '\n')
    if (!/chat\/completions/.test(req.url)) { res.writeHead(404); return res.end('{}') }
    const content = replies[Math.min(i++, replies.length - 1)]
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end(JSON.stringify({ id: 'stub', object: 'chat.completion', choices: [{ index: 0, finish_reason: 'stop', message: { role: 'assistant', content } }], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } }))
  })
}).listen(port, '127.0.0.1', () => console.log('stub model on', port))
