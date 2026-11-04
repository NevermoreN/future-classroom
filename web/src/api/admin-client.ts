import { HttpApi } from './client'
import { ProtocolFault, protocolVersions, type Role } from './protocol'
/** All IDs and revisions are decimal strings. Dates use YYYY-MM-DD; time is
 * minutes since midnight. These are public DTOs, never database encodings. */
export interface AdminDocument<T> { schemaVersion: 1; revision: string; data: T }
export type ComponentKind = 'clock' | 'date' | 'schedule' | 'homework' | 'materials' | 'reminder' | 'date_countdown' | 'recess' | 'subject_text' | 'subject_image'
export interface ComponentStyle { kind: ComponentKind; visible: boolean; opacityPercent: number; fontDip: number; minWidth: number; maxWidth: number; minHeight: number; maxHeight: number }
export interface DisplayData {
  targetMonitor: string | null; clockMode: 'digital24' | 'digital12' | 'analog'; clockSeconds: boolean; recessEnabled: boolean; subjectFill: boolean
  courseFillPercent: number; homeworkFillPercent: number; textTone: 'light' | 'dark'; glassSurfacePercent: number; edgeReflectionPercent: number
  components: ComponentStyle[]; countdown: { target: string | null; label: string; expired: 'hide' | 'show_completed' }
}
export interface IdentityScope { classId: string; subjectId: string }
export interface AdminAccount { id: string; loginName: string; displayName: string; role: Role; enabled: boolean; mustChangePassword: boolean; scopes: IdentityScope[] }
export interface AdminClass { id: string; name: string; enabled: boolean }
export interface AdminSubject { id: string; name: string; color: string; enabled: boolean; revision: string }
export interface IdentityData { recoveryCode?: string;
  schoolName: string; currentClassId: string | null; accounts: AdminAccount[]; classes: AdminClass[]; subjects: AdminSubject[]
  assignments: { teacherId: string; classId: string; subjectId: string; enabled: boolean }[]
  defaultTeachers: { teacherId: string; classId: string; subjectId: string }[]
}
export interface CourseCell { subjectId: string; binding: 'default' | 'explicit' | 'unassigned'; teacherId: string | null }
export interface TimetableRow { id: string; logicalKey: string; group: 'morning' | 'afternoon' | 'other'; groupNumber: number | null; kind: 'course' | 'activity' | 'recess' | 'lunch' | 'rest-other' | 'rest-unknown'; begin: number; end: number; enabledDays: boolean[]; courses: (CourseCell | null)[] }
export interface WeekTable { id: string; role: 'single' | 'odd' | 'even'; rows: TimetableRow[] }
export interface RowDetail { rowId: string; name: string }
export interface CellDetail { rowId: string; day: number; room: string; note: string }
export interface SemesterData { id: string; revision: string; classId: string; name: string; enabled: boolean; start: string; end: string; firstWeekMonday: string; timingSync: boolean; tables: WeekTable[]; rowDetails: RowDetail[]; cellDetails: CellDetail[] }
export interface CalendarEntry { semesterId: string; date: string; mapping: 'rest' | 'unconfirmed' | 'source'; sourceDate: string | null }
export interface CalendarData { coverage: [string, string] | null; verifiedYears: number[]; entries: CalendarEntry[] }
export interface ScheduleData { semesters: SemesterData[]; calendarMode: 'school' | 'public'; automaticGaps: boolean; schoolCalendar: CalendarData; publicCalendar: CalendarData }
export type Axis = { kind: 'fixed'; value: number } | { kind: 'content'; min: number; preferred: number; max: number } | { kind: 'percent'; basisPoints: number; min: number; max: number }
export interface LayoutNode { id: string; children: string[]; kind: 'root' | 'container' | 'outlet' | 'widget'; flow?: 'row' | 'column' | 'wrap'; anchor?: 'top_left' | 'top_right' | 'bottom_left' | 'bottom_right'; padding?: number; gap?: number; width?: Axis; height?: Axis; groupOpacityPercent?: number; material?: { surfacePercent: number; edgePercent: number } | null; widget?: ComponentKind }
export interface LayoutData { nodeIdHighWater: string; root: string; nodes: LayoutNode[]; contentStyles: { nodeId: string; component: ComponentStyle; imageFit: 'contain' }[] }
export interface NetworkData { host: string | null; port: number | null; certificateThumbprint: string | null; sourceNetworks: string[]; listenerState: string; localManagementUrl: string; teacherUrl: string | null; certificateTrusted: boolean | null; certificateExpiresAt: string | null; certificatePublicPem: string | null }
export interface AutostartData { desiredEnabled: boolean; actualState: string; lastError: string | null }
export interface StatusData { applicationVersion: string; schoolName: string | null; currentClassId: string | null; desktopState: string; listenerState: string; storageState: string; diagnostics: { label: string; value: string }[]; monitors: { id: string; name: string; primary: boolean }[] }
export interface VoiceLimits { maxDurationMillis: number; maxPendingJobs: number; maxPendingPerOwner: number; maxRepeatTotal: number }
export interface VoiceSettingsData { publicationRevision: string; identityRevision: string; runtimeGeneration: string; enabled: boolean; admitting: boolean; activeJobs: number; queuedJobs: number; playingJobs: number; limits: VoiceLimits; deviceObservation: { result: 'SUPPORTED' | 'FORMAT_UNSUPPORTED' | 'UNAVAILABLE' | 'FAILED'; observedAt: string; remainingValidityMillis: number } | null }
export interface AdminResources { status: StatusData; identity: IdentityData; display: DisplayData; schedule: ScheduleData; layout: LayoutData; network: NetworkData; autostart: AutostartData; voice: VoiceSettingsData }
export type IdentityCommand =
  | { action: 'renameSchool'; candidate: { name: string } }
  | { action: 'createClass'; candidate: { name: string } }
  | { action: 'updateClass'; candidate: AdminClass }
  | { action: 'bindClass'; candidate: { id: string } }
  | { action: 'putSubject'; candidate: { id: string | null; name: string; color: string; enabled: boolean; expectedRevision: string | null } }
  | { action: 'createAccount'; candidate: { loginName: string; displayName: string; role: Role; password: string; scopes: IdentityScope[] } }
  | { action: 'updateAccount'; candidate: { id: string; displayName: string; role: Role; enabled: boolean; scopes: IdentityScope[] } }
  | { action: 'setDefaultTeacher'; candidate: { teacherId: string; classId: string; subjectId: string } }
export type ScheduleCommand =
  | { action: 'createSemester'; candidate: { classId: string; name: string; start: string; end: string; firstWeekMonday: string } }
  | { action: 'saveSemester'; candidate: SemesterData }
  | { action: 'deleteSemester'; candidate: { id: string } }
  | { action: 'setDualMode'; candidate: { semesterId: string; enabled: boolean; sourceRole: 'single' | 'odd' | 'even'; restorePrevious: boolean } }
  | { action: 'addRow'; candidate: { semesterId: string; tableId: string; kind: TimetableRow['kind']; begin: number; end: number; name: string } }
  | { action: 'saveCalendar'; candidate: { calendarMode: ScheduleData['calendarMode']; automaticGaps: boolean; schoolCalendar: CalendarData; publicCalendar: CalendarData } }
export interface AdminCommands {
  identity: IdentityCommand; display: { action: 'save'; candidate: DisplayData }; layout: { action: 'save'; candidate: LayoutData }
  schedule: ScheduleCommand
  network: { action: 'save'; candidate: { host: string; port: number; certificateThumbprint: string; sourceNetworks: string[] } } | { action: 'generateCertificate'; candidate: { host: string; port: number; sourceNetworks: string[] } } | { action: 'start' | 'stop'; candidate: Record<string, never> }
  voice: { action: 'setEnabled'; candidate: { expectedIdentityRevision: string; expectedRuntimeGeneration: string; enabled: boolean; reviewed: true } } | { action: 'setLimits'; candidate: { expectedIdentityRevision: string; expectedRuntimeGeneration: string; limits: VoiceLimits; reviewed: true } } | { action: 'checkOutput'; candidate: { expectedIdentityRevision: string; expectedRuntimeGeneration: string; expectedPublicationRevision: string } }
  autostart: { action: 'set'; candidate: { enabled: boolean } } | { action: 'repair'; candidate: Record<string, never> }
}
/** Typed envelopes are checked at the public boundary before the structure is
 * used. Each resource's detailed validator below checks its bounded DTO. */
function document<R extends keyof AdminResources>(value: unknown, resource: R): AdminDocument<AdminResources[R]> {
  return adminEnvelope(value, data => validators[resource](data) as AdminResources[R])
}
export function adminEnvelope<T>(value: unknown, validate: (data: unknown) => T): AdminDocument<T> {
  if (!record(value) || Object.keys(value).sort().join(',') !== 'data,revision,schemaVersion') throw new ProtocolFault()
  protocolVersions(value.schemaVersion)
  id(value.revision, true)
  return { schemaVersion: 1, revision: value.revision as string, data: validate(value.data) }
}
export function parseLayoutData(value: unknown): LayoutData { return validators.layout(value) as LayoutData }
export function parseComponentStyle(value: unknown): ComponentStyle { component(value); return value as ComponentStyle }
function record(v: unknown): v is Record<string, unknown> { return typeof v === 'object' && v !== null && !Array.isArray(v) }
function id(v: unknown, zero = false): asserts v is string { if (typeof v !== 'string' || !(zero ? /^(0|[1-9][0-9]{0,19})$/ : /^[1-9][0-9]{0,19}$/).test(v) || BigInt(v) > 18446744073709551615n) throw new ProtocolFault() }
function str(v: unknown, max = 4096): asserts v is string { if (typeof v !== 'string' || [...v].length > max || /[\u0000-\u0008\u000b\u000c\u000e-\u001f]/.test(v)) throw new ProtocolFault() }
function num(v: unknown, max = 65535): asserts v is number { if (typeof v !== 'number' || !Number.isSafeInteger(v) || v < 0 || v > max) throw new ProtocolFault() }
function bool(v: unknown): asserts v is boolean { if (typeof v !== 'boolean') throw new ProtocolFault() }
function list(v: unknown, max: number): asserts v is unknown[] { if (!Array.isArray(v) || v.length > max) throw new ProtocolFault() }
function object(v: unknown): asserts v is Record<string, unknown> { if (!record(v)) throw new ProtocolFault() }
function one(v: unknown, allowed: readonly string[]) { if (typeof v !== 'string' || !allowed.includes(v)) throw new ProtocolFault() }
const componentKinds = ['clock', 'date', 'schedule', 'homework', 'materials', 'reminder', 'date_countdown', 'recess', 'subject_text', 'subject_image'] as const
function component(v: unknown) { object(v); one(v.kind, componentKinds); bool(v.visible); num(v.opacityPercent, 100); for (const k of ['fontDip', 'minWidth', 'maxWidth', 'minHeight', 'maxHeight']) num(v[k], 16384) }
function scope(v: unknown) { object(v); id(v.classId); id(v.subjectId) }
function axis(v: unknown) { object(v); one(v.kind, ['fixed', 'content', 'percent']); if (v.kind === 'fixed') num(v.value, 16384); else { num(v.min, 16384); num(v.max, 16384); if (v.kind === 'content') num(v.preferred, 16384); else num(v.basisPoints, 10000) } }
function calendar(v: unknown) { object(v); if (v.coverage !== null) { list(v.coverage, 2); if (v.coverage.length !== 2) throw new ProtocolFault(); v.coverage.forEach(x => str(x, 10)) } list(v.verifiedYears, 32); v.verifiedYears.forEach(x => num(x, 9999)); list(v.entries, 4096); v.entries.forEach(x => { object(x); id(x.semesterId); str(x.date, 10); one(x.mapping, ['rest', 'unconfirmed', 'source']); if (x.sourceDate !== null) str(x.sourceDate, 10) }) }
const validators: Record<keyof AdminResources, (value: unknown) => unknown> = {
  status(v) { object(v); str(v.applicationVersion); if (v.schoolName !== null) str(v.schoolName); if (v.currentClassId !== null) id(v.currentClassId); for (const k of ['desktopState', 'listenerState', 'storageState']) str(v[k]); list(v.diagnostics, 32); v.diagnostics.forEach(x => { object(x); str(x.label, 64); str(x.value, 512) }); list(v.monitors, 128); v.monitors.forEach(x => { object(x); str(x.id); str(x.name); bool(x.primary) }); return v },
  identity(v) { object(v); if (v.recoveryCode !== undefined && (typeof v.recoveryCode !== 'string' || !/^FC1-[0-9a-f]{64}$/.test(v.recoveryCode))) throw new ProtocolFault(); str(v.schoolName); if (v.currentClassId !== null) id(v.currentClassId); list(v.accounts, 512); list(v.classes, 128); list(v.subjects, 256); list(v.assignments, 4096); list(v.defaultTeachers, 4096); v.accounts.forEach(x => { object(x); id(x.id); str(x.loginName, 64); str(x.displayName, 128); one(x.role, ['GLOBAL_ADMIN', 'TEACHER_ADMIN', 'USER']); bool(x.enabled); bool(x.mustChangePassword); list(x.scopes, 4096); x.scopes.forEach(scope) }); v.classes.forEach(x => { object(x); id(x.id); str(x.name); bool(x.enabled) }); v.subjects.forEach(x => { object(x); id(x.id); id(x.revision, true); str(x.name); str(x.color, 7); if (!/^#[0-9a-f]{6}$/i.test(x.color)) throw new ProtocolFault(); bool(x.enabled) }); v.assignments.forEach(x => { scope(x); object(x); id(x.teacherId); bool(x.enabled) }); v.defaultTeachers.forEach(x => { scope(x); object(x); id(x.teacherId) }); return v },
  display(v) { object(v); if (v.targetMonitor !== null) str(v.targetMonitor); one(v.clockMode, ['digital24', 'digital12', 'analog']); one(v.textTone, ['light', 'dark']); for (const k of ['clockSeconds', 'recessEnabled', 'subjectFill']) bool(v[k]); for (const k of ['courseFillPercent', 'homeworkFillPercent', 'glassSurfacePercent', 'edgeReflectionPercent']) num(v[k], 100); list(v.components, 8); v.components.forEach(component); object(v.countdown); if (v.countdown.target !== null) str(v.countdown.target, 10); str(v.countdown.label, 256); one(v.countdown.expired, ['hide', 'show_completed']); return v },
  schedule(v) { object(v); one(v.calendarMode, ['school', 'public']); bool(v.automaticGaps); calendar(v.schoolCalendar); calendar(v.publicCalendar); list(v.semesters, 32); v.semesters.forEach(s => { object(s); id(s.id); id(s.revision, true); id(s.classId); str(s.name); bool(s.enabled); bool(s.timingSync); for (const k of ['start', 'end', 'firstWeekMonday']) str(s[k], 10); list(s.tables, 2); s.tables.forEach(t => { object(t); id(t.id); one(t.role, ['single', 'odd', 'even']); list(t.rows, 128); t.rows.forEach(r => { object(r); id(r.id); id(r.logicalKey); one(r.kind, ['course', 'activity', 'recess', 'lunch', 'rest-other', 'rest-unknown']); one(r.group, ['morning', 'afternoon', 'other']); if (r.groupNumber !== null) num(r.groupNumber); num(r.begin, 1440); num(r.end, 1440); list(r.enabledDays, 7); list(r.courses, 7); if (r.enabledDays.length !== 7 || r.courses.length !== 7) throw new ProtocolFault(); r.enabledDays.forEach(bool); r.courses.forEach(c => { if (c === null) return; object(c); id(c.subjectId); one(c.binding, ['default', 'explicit', 'unassigned']); if (c.teacherId !== null) id(c.teacherId) }) }) }); list(s.rowDetails, 4096); s.rowDetails.forEach(d => { object(d); id(d.rowId); str(d.name) }); list(s.cellDetails, 4096); s.cellDetails.forEach(d => { object(d); id(d.rowId); num(d.day, 6); str(d.room); str(d.note) }) }); return v },
  layout(v) { object(v); id(v.nodeIdHighWater, true); id(v.root); list(v.nodes, 128); v.nodes.forEach(n => { object(n); id(n.id); list(n.children, 128); n.children.forEach(x => id(x)); one(n.kind, ['root', 'container', 'outlet', 'widget']); if (n.kind === 'widget') one(n.widget, componentKinds); else { axis(n.width); axis(n.height); if (n.kind !== 'outlet') { one(n.flow, ['row', 'column', 'wrap']); num(n.padding, 16384); num(n.gap, 16384); if (n.kind === 'root') one(n.anchor, ['top_left', 'top_right', 'bottom_left', 'bottom_right']); else { num(n.groupOpacityPercent, 100); if (n.material !== null) { object(n.material); num(n.material.surfacePercent, 100); num(n.material.edgePercent, 100) } } } } }); list(v.contentStyles, 128); v.contentStyles.forEach(s => { object(s); id(s.nodeId); component(s.component); one(s.imageFit, ['contain']) }); return v },
  network(v) { object(v); for (const k of ['host', 'certificateThumbprint', 'teacherUrl', 'certificateExpiresAt']) if (v[k] !== null) str(v[k]); if (v.certificatePublicPem !== null) str(v.certificatePublicPem, 65536); if (v.port !== null) num(v.port); list(v.sourceNetworks, 32); v.sourceNetworks.forEach(x => str(x, 128)); str(v.listenerState); str(v.localManagementUrl); if (v.certificateTrusted !== null) bool(v.certificateTrusted); return v },
  voice(v) { object(v); for (const key of ['publicationRevision', 'identityRevision', 'runtimeGeneration']) id(v[key], true); bool(v.enabled); bool(v.admitting); for (const key of ['activeJobs', 'queuedJobs', 'playingJobs']) num(v[key], 104); object(v.limits); num(v.limits.maxDurationMillis, 60000); num(v.limits.maxPendingJobs, 4); num(v.limits.maxPendingPerOwner, 2); num(v.limits.maxRepeatTotal, 10); if (v.deviceObservation !== null) { object(v.deviceObservation); one(v.deviceObservation.result, ['SUPPORTED', 'FORMAT_UNSUPPORTED', 'UNAVAILABLE', 'FAILED']); str(v.deviceObservation.observedAt, 64); num(v.deviceObservation.remainingValidityMillis, Number.MAX_SAFE_INTEGER) } return v },
  autostart(v) { object(v); bool(v.desiredEnabled); str(v.actualState); if (v.lastError !== null) str(v.lastError); return v },
}
export class AdminHttpApi extends HttpApi {
  async read<R extends keyof AdminResources>(resource: R, signal: AbortSignal): Promise<AdminDocument<AdminResources[R]>> {
    const result = await this.request('admin/' + resource, signal, undefined, undefined, 4 * 1024 * 1024)
    if (result.status !== 200) throw new ProtocolFault()
    return document(result.value, resource)
  }
  async write<R extends keyof AdminCommands>(resource: R, expectedRevision: string, command: AdminCommands[R], csrf: string, signal: AbortSignal): Promise<AdminDocument<AdminResources[R]>> {
    id(expectedRevision, true)
    const result = await this.request('admin/' + resource, signal, { schemaVersion: 1, expectedRevision, ...command }, csrf, 4 * 1024 * 1024)
    if (result.status !== 200) throw new ProtocolFault()
    return document(result.value, resource)
  }
}
