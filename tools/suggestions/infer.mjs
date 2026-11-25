// The parent consumes this pipe privately; it never forwards raw model output.
for (const method of ['log', 'info', 'warn', 'error', 'debug', 'trace']) console[method] = () => {}
process.on('uncaughtException', () => process.exit(1))
process.on('unhandledRejection', () => process.exit(1))

const { AutoProcessor, Florence2ForConditionalGeneration, env, load_image } = await import('@huggingface/transformers')
env.allowRemoteModels = false
env.allowLocalModels = true
const root = '/opt/florence-model'
const model = await Florence2ForConditionalGeneration.from_pretrained(root, {
  dtype: 'q8', device: 'cpu', local_files_only: true,
  session_options: { intraOpNumThreads: 2, interOpNumThreads: 1 },
})
const processor = await AutoProcessor.from_pretrained(root, { local_files_only: true })

function send(value) {
  const body = Buffer.from(JSON.stringify(value))
  const size = Buffer.alloc(4)
  size.writeUInt32BE(body.length)
  process.stdout.write(Buffer.concat([size, body]))
}

async function infer(bytes) {
  const image = await load_image(new Blob([bytes], { type: 'image/jpeg' }))
  const results = []
  for (const task of ['<OD>', '<DENSE_REGION_CAPTION>', '<MORE_DETAILED_CAPTION>']) {
    const inputs = await processor(image, processor.construct_prompts(task))
    const generated = await model.generate({ ...inputs, max_new_tokens: 256, num_beams: 3, do_sample: false })
    const text = processor.batch_decode(generated, { skip_special_tokens: false })[0]
    results.push(processor.post_process_generation(text, task, image.size)[task])
  }
  return { od: results[0].labels, dense: results[1].labels, caption: results[2] }
}

send({ ready: true })
let pending = Buffer.alloc(0)
for await (const chunk of process.stdin) {
  pending = Buffer.concat([pending, chunk])
  while (pending.length >= 4) {
    const length = pending.readUInt32BE()
    if (length < 1 || length > 8 * 1024 * 1024) process.exit(1)
    if (pending.length < length + 4) break
    const bytes = pending.subarray(4, length + 4)
    pending = pending.subarray(length + 4)
    try {
      send(await infer(bytes))
    } catch {
      send({ error: true })
    }
  }
}
