import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
export function Field({ label, children }: { label: string; children: ReactNode }) { return <label className="admin-field"><span>{label}</span>{children}</label> }
export function NumberField({ label, value, onChange, min = 0, max = 16384 }: { label: string; value: number; onChange: (value: number) => void; min?: number; max?: number }) { return <Field label={label}><Input type="number" min={min} max={max} value={value} onChange={e => { if (e.target.value !== '') onChange(Number(e.target.value)) }} required /></Field> }
export function Toggle({ label, value, onChange }: { label: string; value: boolean; onChange: (value: boolean) => void }) { return <label className="admin-toggle"><input type="checkbox" checked={value} onChange={e => onChange(e.target.checked)} /><span>{label}</span></label> }
export function Section({ title, description, children }: { title: string; description?: string; children: ReactNode }) { return <Card className="admin-section"><CardHeader><CardTitle>{title}</CardTitle>{description && <CardDescription>{description}</CardDescription>}</CardHeader><CardContent>{children}</CardContent></Card> }
