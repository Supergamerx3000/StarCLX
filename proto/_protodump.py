import re,glob,base64,os,sys
from google.protobuf import descriptor_pb2 as d
src=sys.argv[1]; out=sys.argv[2]
T={1:'double',2:'float',3:'int64',4:'uint64',5:'int32',6:'fixed64',7:'fixed32',8:'bool',9:'string',12:'bytes',13:'uint32',15:'sfixed32',16:'sfixed64',17:'sint32',18:'sint64'}
def typ(f): return f.type_name if f.type in (11,14) else T[f.type]
def msg(m,ind,maps):
    s=[]; p='  '*ind
    nested={n.name:n for n in m.nested_type}
    s.append(f'{p}message {m.name} {{')
    for e in m.enum_type: s+=enum(e,ind+1)
    for n in m.nested_type:
        if n.options.map_entry: continue
        s+=msg(n,ind+1,maps)
    oneofs={}
    for f in m.field:
        if f.HasField('oneof_index') and not f.proto3_optional: oneofs.setdefault(f.oneof_index,[]).append(f)
    done=set()
    for f in m.field:
        if f.HasField('oneof_index') and not f.proto3_optional:
            if f.oneof_index in done: continue
            done.add(f.oneof_index)
            s.append(f'{p}  oneof {m.oneof_decl[f.oneof_index].name} {{')
            for g in oneofs[f.oneof_index]: s.append(f'{p}    {typ(g)} {g.name} = {g.number};')
            s.append(f'{p}  }}'); continue
        tn=f.type_name.split('.')[-1]
        if f.type==11 and tn in nested and nested[tn].options.map_entry:
            k,v=nested[tn].field; s.append(f'{p}  map<{typ(k)}, {typ(v)}> {f.name} = {f.number};'); continue
        lab='repeated ' if f.label==3 else ('optional ' if f.proto3_optional else '')
        s.append(f'{p}  {lab}{typ(f)} {f.name} = {f.number};')
    s.append(f'{p}}}'); return s
def enum(e,ind):
    p='  '*ind; return [f'{p}enum {e.name} {{']+[f'{p}  {v.name} = {v.number};' for v in e.value]+[f'{p}}}']
n=0
for fn in glob.glob(src+'/**/*Reflection.cs',recursive=True):
    t=open(fn).read()
    m=re.search(r'FromBase64String\((.*?)\),\s*new FileDescriptor',t,re.S)
    if not m: continue
    b=''.join(re.findall(r'"([^"]*)"',m.group(1)))
    fd=d.FileDescriptorProto(); fd.ParseFromString(base64.b64decode(b))
    L=[f'syntax = "{fd.syntax or "proto2"}";',f'package {fd.package};']+[f'import "{i}";' for i in fd.dependency]+['']
    for sv in fd.service:
        L.append(f'service {sv.name} {{')
        for me in sv.method:
            L.append(f'  rpc {me.name}({"stream " if me.client_streaming else ""}{me.input_type}) returns ({"stream " if me.server_streaming else ""}{me.output_type});')
        L.append('}\n')
    for e in fd.enum_type: L+=enum(e,0)
    for mm in fd.message_type: L+=msg(mm,0,{})+['']
    path=os.path.join(out,fd.name); os.makedirs(os.path.dirname(path),exist_ok=True)
    open(path,'w').write('\n'.join(L)+'\n'); n+=1
print(n,'files')
