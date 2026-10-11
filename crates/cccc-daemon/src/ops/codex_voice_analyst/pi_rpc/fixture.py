#!/usr/bin/env python3
"""Offline native Pi RPC fixture. No accounts, sockets, or model calls."""
import json, os, pathlib, sys, uuid
if '--version' in sys.argv:
    print(os.environ.get('PI_FIXTURE_VERSION','1.1.0'));sys.exit(0)
def option(name,default=None):
    return sys.argv[sys.argv.index(name)+1] if name in sys.argv else default
session_file=option('--session')
if session_file:
    sid=json.loads(pathlib.Path(session_file).read_text().splitlines()[0])['id']
else:
    sid=option('--session-id', 'fixture-session')
    directory=pathlib.Path(option('--session-dir',os.environ.get('PI_FIXTURE_DIRECTORY','.')))
    directory.mkdir(parents=True,exist_ok=True)
    session_file=str(directory/f'fixture_{sid}.jsonl')
active=None;queued=False;clear_waiting=None;preflight_request=None
def emit(value):print(json.dumps(value,ensure_ascii=False),flush=True)
def response(request,data=None,success=True):emit({'type':'response','id':request['id'],'command':request['type'],'success':success,'data':data or {}})
def final(failed=False):
    emit({'type':'message_start','message':{'role':'assistant','content':[]}})
    emit({'type':'message_update','assistantMessageEvent':{'type':'text_delta','contentIndex':0,'delta':'partial'}})
    emit({'type':'message_end','message':{'role':'assistant','content':[{'type':'text','text':'authoritative 世界'}],'stopReason':'error' if failed else 'stop','errorMessage':'private fixture provider error' if failed else None}})
    emit({'type':'agent_end','messages':[]});emit({'type':'agent_settled','aborted':False})
for line in sys.stdin:
    r=json.loads(line);typ=r['type']
    log=os.environ.get('PI_FIXTURE_WIRE')
    if log:
        with open(log,'a') as f:f.write(json.dumps({'type':typ})+'\n')
    if typ=='get_state':response(r,{'sessionId':sid,'sessionFile':session_file,'isStreaming':active is not None,'isCompacting':False,'pendingMessageCount':int(queued)})
    elif typ=='prompt':
        text=r['message'];active=text
        if text=='handled':response(r,{'disposition':'handled'});active=None;continue
        if text=='queued':
            response(r,{'disposition':'queued'});queued=True;continue
        if text=='accepted-only':response(r,{'disposition':'started'});continue
        if text=='preflight-dialog':
            preflight_request=r
            emit({'type':'extension_ui_request','id':'preflight-ui','method':'select','title':'Fixture preflight','options':['one','two']});continue
        if text.startswith('dialog'):
            emit({'type':'message_start','message':{'role':'user','content':[{'type':'text','text':text}]}})
            response(r,{'disposition':'started'})
            emit({'type':'extension_ui_request','id':'reused-provider-id','method':'select','title':'Fixture choice','options':['one','two']});continue
        pathlib.Path(session_file).write_text(json.dumps({'type':'session','version':3,'id':sid,'cwd':str(pathlib.Path.cwd())})+'\n')
        emit({'type':'message_start','message':{'role':'user','content':[{'type':'text','text':'not-the-owned-prompt' if text=='foreign' else text}]}})
        response(r,{'disposition':'started'})
        if text not in ('hold','hold-late','foreign'):final(text=='failed');active=None
    elif typ=='extension_ui_response':
        if r.get('cancelled'):
            if r['id']=='late-before-ack':
                response(clear_waiting,{'steering':[],'followUp':[]});clear_waiting=None
                emit({'type':'extension_ui_request','id':'late-after-ack','method':'editor','title':'Late fixture editor','prefill':'fixture'})
            elif r['id']=='preflight-ui':pass
            elif not r['id'].startswith('late-'):active=None
        else:
            emit({'type':'message_start','message':{'role':'user','content':[{'type':'text','text':active}]}})
            # Dialog occurs in before_agent_start before prompt response.
            # The fixture does not need the response to complete observable work.
            final();active=None
    elif typ=='clear_queue':
        if active=='hold-late':
            clear_waiting=r
            emit({'type':'extension_ui_request','id':'late-before-ack','method':'input','title':'Late fixture input','placeholder':'fixture'})
            continue
        if queued:
            # Adversarial ordering: unrelated settlement arrives before queue ack.
            emit({'type':'agent_settled','aborted':False});queued=False;active=None
        response(r,{'steering':[],'followUp':[]})
    elif typ=='abort':
        if active=='preflight-dialog':
            response(r)
            emit({'type':'message_start','message':{'role':'user','content':[{'type':'text','text':active}]}})
            response(preflight_request,{'disposition':'started'})
            emit({'type':'agent_start'})
            active=None;continue
        if active is not None:emit({'type':'agent_settled','aborted':True})
        active=None;response(r)
    else:response(r)
