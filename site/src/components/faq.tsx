import { questions } from '@/content/faq'

export function Faq() {
  return (
    <div>
      {questions.map((item) => (
        <details key={item.question} className="faq-item border-b border-line">
          <summary className="flex cursor-pointer list-none items-center justify-between gap-4 py-5 text-base font-medium">
            {item.question}
            <span className="faq-plus relative size-3 shrink-0 text-faint" aria-hidden="true" />
          </summary>
          <p className="animate-rise-fast pr-8 pb-5.5 text-[15px]/[1.6] text-muted">{item.answer}</p>
        </details>
      ))}
    </div>
  )
}
