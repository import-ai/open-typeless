import { useRef, type ReactNode } from 'react'
import { Dialog as Primitive } from 'radix-ui'
import { X } from 'lucide-react'
import { Button } from '@/components/ui/button'

export function Dialog({ open, onOpenChange, title, description, children, busy = false, onCloseAutoFocus }: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  children: ReactNode
  busy?: boolean
  onCloseAutoFocus?: (event: Event) => void
}) {
  const content = useRef<HTMLDivElement>(null)
  return <Primitive.Root open={open} onOpenChange={value => { if (!busy) onOpenChange(value) }}>
    <Primitive.Portal>
      <Primitive.Overlay className="fixed inset-0 z-40 bg-black/40" />
      <Primitive.Content
        ref={content}
        className="fixed left-1/2 top-1/2 z-50 w-[calc(100%_-_3rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 rounded-2xl border bg-background p-5 shadow-xl outline-none"
        onCloseAutoFocus={onCloseAutoFocus}
        onOpenAutoFocus={event => {
          const target = content.current?.querySelector<HTMLElement>('[data-dialog-autofocus]')
          if (target) { event.preventDefault(); target.focus() }
        }}
        onEscapeKeyDown={event => { if (event.isComposing || event.keyCode === 229) event.preventDefault() }}
      >
        <Primitive.Title className="pr-8 text-lg font-semibold">{title}</Primitive.Title>
        <Primitive.Description className="mt-2 text-sm text-muted-foreground">{description}</Primitive.Description>
        <Primitive.Close asChild><Button variant="ghost" size="icon" className="absolute right-3 top-3" aria-label="关闭" disabled={busy}><X /></Button></Primitive.Close>
        {children}
      </Primitive.Content>
    </Primitive.Portal>
  </Primitive.Root>
}
