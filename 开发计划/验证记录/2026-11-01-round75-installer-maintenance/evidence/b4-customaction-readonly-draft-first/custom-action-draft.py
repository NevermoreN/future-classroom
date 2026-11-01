"""Pure B4 authoring proposal only. No tools/MSI/Windows or frozen source edits.
Avoid unsupported wixl nodes and references together. Import the exact CA and
three execution rows after successful warning-free intermediate compilation.
"""
from pathlib import Path
import runpy, xml.etree.ElementTree as ET
ROOT=Path('/tmp/fc-round75-installer-maintenance/worktree')
M=runpy.run_path(str(ROOT/'scripts/package-msi-candidate.py'))

def custom_action_idt():
    lines=['Action\tType\tSource\tTarget','s72\ti2\tS72\tS255','CustomAction\tAction']
    for name,(kind,target,_) in M['LIFECYCLE_ACTIONS'].items():
        lines.append('\t'.join([name,str(kind),M['HELPER_BINARY'],target]))
    return ('\r\n'.join(lines)+'\r\n').encode('ascii')

def compiler_authoring(version,names):
    """Only unexecuted intermediate WXS; final MSI is defined by checked tables."""
    root=ET.fromstring(M['authoring'](version,names));ns='{'+M['WIX_NS']+'}'
    product=root.find(ns+'Product');assert product is not None
    for child in list(product):
        if child.tag==ns+'CustomAction':product.remove(child)
        elif child.tag==ns+'InstallExecuteSequence':
            for row in list(child):
                if row.tag==ns+'Custom' and row.get('Action') in M['LIFECYCLE_ACTIONS']:child.remove(row)
    ET.indent(root)
    return ET.tostring(root,encoding='utf-8',xml_declaration=True)+b'\n'

def execution_idt(intermediate):
    rows=M['idt_rows'](intermediate,'InstallExecuteSequence')
    if any(set(row)!={'Action','Condition','Sequence'} for row in rows):raise ValueError('schema')
    if any(row['Action'] in M['LIFECYCLE_ACTIONS'] for row in rows):raise ValueError('intermediate_has_unexpected_lifecycle_action')
    full=rows+[{'Action':name,'Condition':'1','Sequence':str(spec[2])} for name,spec in M['LIFECYCLE_ACTIONS'].items()]
    full.sort(key=lambda row:row['Action'])
    lines=['Action\tCondition\tSequence','s72\tS255\ti2','InstallExecuteSequence\tAction']
    for row in full:
        if any('\t' in v or '\r' in v or '\n' in v for v in row.values()):raise ValueError('cell_delimiter')
        lines.append('\t'.join(row[k] for k in ('Action','Condition','Sequence')))
    raw=('\r\n'.join(lines)+'\r\n').encode('ascii')
    M['verify_transaction_sequence'](raw)
    return raw
