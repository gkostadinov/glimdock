"""Synthetic local handoff tests. Never read a real archive, key or launch URL."""
from __future__ import annotations
import argparse
from email.message import Message
import http.client
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys
import tempfile
from threading import Thread
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import build_opener,ProxyHandler

sys.path.insert(0,str(Path(__file__).parent))
import api


def settings(private=False,bind='127.0.0.1'):
    return argparse.Namespace(bind=bind,port=0,hub=None,snapshot=None,read_token_file=None,
                              include_task_titles=False,private_transcripts=private)


class RunningServer:
    def __init__(self,server):self.server=server
    def __enter__(self):
        self.thread=Thread(target=self.server.serve_forever,kwargs={'poll_interval':.01},daemon=True)
        self.thread.start()
        return self.server
    def __exit__(self,*args):
        self.server.shutdown();self.server.server_close();self.thread.join(timeout=2)


class PrivateKeyTests(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory()
        self.path=Path(self.directory.name)/'reader.key'
        self.path.write_bytes(b's'*48);self.path.chmod(0o600)
    def tearDown(self):self.directory.cleanup()

    def test_valid_private_file_is_read_without_any_output(self):
        self.assertTrue(api.private_key(self.path)==('s'*48))

    def test_permissions_owner_symlinks_and_nonregular_files_are_rejected(self):
        for mode in (0o400,0o640,0o644,0o660,0o700):
            self.path.chmod(mode)
            with self.assertRaises((OSError,ValueError)):api.private_key(self.path)
        self.path.chmod(0o600)
        with patch.object(api.os,'getuid',return_value=os.getuid()+1):
            with self.assertRaises(ValueError):api.private_key(self.path)
        link=Path(self.directory.name)/'link';link.symlink_to(self.path)
        with self.assertRaises((OSError,ValueError)):api.private_key(link)
        fifo=Path(self.directory.name)/'fifo';os.mkfifo(fifo,0o600)
        with self.assertRaises((OSError,ValueError)):api.private_key(fifo)
        with self.assertRaises((OSError,ValueError)):api.private_key(Path(self.directory.name))

    def test_file_bounds_and_header_injection_are_rejected(self):
        for content in (b's'*23,b's'*4097,b's'*48+b'\n',b's'*48+b'\r',b's'*30+b'\x00',b's'*30+b' ',b's'*30+b'\xff'):
            self.path.write_bytes(content)
            with self.assertRaises((OSError,ValueError)):api.private_key(self.path)
        for length in (24,4096):
            self.path.write_bytes(b's'*length)
            self.assertTrue(len(api.private_key(self.path))==length)


class RequestPolicyTests(unittest.TestCase):
    host='127.0.0.1:43210'
    def headers(self,**values):
        headers=Message();headers['Host']=self.host
        for name,value in values.items():headers[name.replace('_','-')]=value
        return headers

    def test_normal_navigation_and_exact_same_origin_are_allowed(self):
        for values in ({},{'Sec_Fetch_Site':'none'},
                       {'Origin':'http://'+self.host,'Referer':'http://'+self.host+'/nodes/?view=sample','Sec_Fetch_Site':'same-origin'}):
            self.assertTrue(api.private_request_allowed('127.0.0.1',self.headers(**values),self.host))

    def test_wrong_peer_host_port_origin_referrer_and_fetch_site_are_blocked(self):
        for peer in ('192.0.2.10','0.0.0.0','192.0.2.10','invalid'):
            self.assertFalse(api.private_request_allowed(peer,self.headers(),self.host))
        for host in ('evil.example:43210','127.0.0.1','127.0.0.1:43211','localhost:43210',self.host+'.evil.example'):
            headers=self.headers();headers.replace_header('Host',host)
            self.assertFalse(api.private_request_allowed('127.0.0.1',headers,self.host))
        for field,values in (('Origin',('null','https://'+self.host,'http://evil.example','http://'+self.host+'/',
                                      'http://'+self.host+':80')),
                             ('Referer',('http://evil.example/','http://'+self.host+'@evil.example/','https://'+self.host+'/',
                                        ' http://'+self.host+'/')),
                             ('Sec_Fetch_Site',('same-site','cross-site','', 'Same-Origin'))):
            for value in values:
                self.assertFalse(api.private_request_allowed('127.0.0.1',self.headers(**{field:value}),self.host))
        for field in ('Host','Origin','Referer','Sec-Fetch-Site'):
            headers=self.headers();headers[field]=self.host if field=='Host' else ''
            headers[field]='duplicate'
            self.assertFalse(api.private_request_allowed('127.0.0.1',headers,self.host))


class BrokerTests(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory()
        self.key=Path(self.directory.name)/'reader.key'
        self.key.write_bytes(b's'*48);self.key.chmod(0o600)
        self.calls=[]
        self.body=json.dumps({'ticket':'t'*48,'expires_in':60,'one_use':True}).encode()
        self.status=200
        outer=self
        class Broker(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_POST(self):
                # Keep only non-secret observations; never persist Authorization.
                outer.calls.append({'method':self.command,'path':self.path,
                                    'auth_ok':self.headers.get('Authorization')=='Bearer '+('s'*48),
                                    'body_ok':self.rfile.read(int(self.headers.get('Content-Length',0)))==b'{}'})
                self.send_response(outer.status)
                if outer.status==302:self.send_header('Location','http://127.0.0.1:1/forbidden')
                self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(outer.body)))
                self.end_headers();self.wfile.write(outer.body)
        self.broker=ThreadingHTTPServer(('127.0.0.1',0),Broker)
        self.running=RunningServer(self.broker);self.running.__enter__()
        self.origin='http://127.0.0.1:'+str(self.broker.server_port)
        self.patch_key=patch.object(api,'PRIVATE_READER_KEY',self.key)
        self.patch_origin=patch.object(api,'PRIVATE_READER_ORIGIN',self.origin)
        self.patch_key.start();self.patch_origin.start()
    def tearDown(self):
        self.patch_origin.stop();self.patch_key.stop();self.running.__exit__();self.directory.cleanup()

    def test_fixed_bounded_post_bypasses_environment_proxy(self):
        captured={}
        real_builder=build_opener
        def builder(*handlers):
            captured['explicit_no_proxy']=any(isinstance(handler,ProxyHandler) and handler.proxies=={} for handler in handlers)
            opener=real_builder(*handlers)
            real_open=opener.open
            def bounded_open(request,timeout):
                captured['timeout']=timeout
                return real_open(request,timeout=timeout)
            opener.open=bounded_open
            return opener
        with patch.dict(os.environ,{'http_proxy':'http://127.0.0.1:1','HTTP_PROXY':'http://127.0.0.1:1','no_proxy':'','NO_PROXY':''}),patch.object(api,'build_opener',side_effect=builder):
            self.assertTrue(api.private_launch_ticket()==('t'*48))
        self.assertTrue(captured['explicit_no_proxy'])
        self.assertTrue(captured['timeout']==3)
        self.assertTrue(self.calls==[{'method':'POST','path':'/v1/launch-ticket','auth_ok':True,'body_ok':True}])

    def test_redirects_invalid_json_oversize_and_bad_ticket_contracts_are_rejected(self):
        self.status=302
        with self.assertRaises(HTTPError):api.private_launch_ticket()
        self.assertEqual(len(self.calls),1)
        self.status=200
        cases=[b'x'*2049,b'{',b'[]',b'{"ticket":"a","ticket":"b"}',b'{"ticket":NaN}']
        for changed in ({'ticket':'t'*23},{'ticket':'t'*193},{'ticket':('t'*30)+'/'},
                        {'ticket':True},{'one_use':False},{'expires_in':61},{'expires_in':True},{'expires_in':0}):
            cases.append(json.dumps({'ticket':'t'*48,'expires_in':60,'one_use':True,**changed}).encode())
        for raw in cases:
            self.body=raw
            with self.assertRaises((OSError,ValueError)):api.private_launch_ticket()

    def request(self,server,path,headers=None,method='GET'):
        connection=http.client.HTTPConnection('127.0.0.1',server.server_port,timeout=3)
        connection.request(method,path,headers=headers or {})
        response=connection.getresponse();status=response.status;response_headers=dict(response.getheaders());body=response.read()
        connection.close()
        return status,response_headers,body

    def test_enabled_handoff_is_303_with_private_headers_and_no_saved_location(self):
        with RunningServer(api.create_server(settings(True))) as server:
            status,headers,body=self.request(server,'/private-transcripts')
            self.assertTrue(status==303)
            self.assertTrue(headers['Location']==self.origin+'/#ticket='+('t'*48))
            self.assertEqual(headers['Cache-Control'],'no-store')
            self.assertEqual(headers['Referrer-Policy'],'no-referrer')
            self.assertEqual(body,b'')
            self.assertFalse(any(('t'*48) in str(value) for value in vars(server).values()))
            for path in ('/','/nodes','/nodes/'):
                status,_,body=self.request(server,path)
                self.assertTrue(status==200 and b'Private conversations' in body and b'/private-transcripts' in body)
                self.assertFalse(('t'*48).encode() in body)
            _,_,body=self.request(server,'/v1/snapshot?mode=demo')
            self.assertFalse(b'Private conversations' in body or b'ticket=' in body or ('s'*48).encode() in body)

    def test_forbidden_requests_do_not_contact_broker_and_failures_are_generic(self):
        with RunningServer(api.create_server(settings(True))) as server:
            for headers in ({'Host':'evil.example:'+str(server.server_port)}, {'Origin':'http://evil.example'},
                            {'Referer':'http://evil.example/'},{'Sec-Fetch-Site':'same-site'},{'Sec-Fetch-Site':'cross-site'}):
                status,response_headers,body=self.request(server,'/private-transcripts',headers)
                self.assertEqual(status,403)
                self.assertNotIn('Location',response_headers)
            for path in ('/private-transcripts?','/private-transcripts?mode=private','/private-transcripts/','/private-transcripts#bad',
                         '//private-transcripts','///private-transcripts'):
                self.assertEqual(self.request(server,path)[0],404)
            self.assertEqual(self.request(server,'/private-transcripts',method='POST')[0],405)
            self.assertEqual(self.calls,[])
            self.body=b'x'*2049
            status,headers,body=self.request(server,'/private-transcripts')
            self.assertEqual(status,503)
            self.assertEqual(json.loads(body),{'error':'private_reader_unavailable'})
            self.assertNotIn('Location',headers)
            self.assertEqual(headers['Cache-Control'],'no-store')
            self.assertEqual(headers['Referrer-Policy'],'no-referrer')

    def test_disabled_server_has_no_action_and_never_reads_a_private_key(self):
        with patch.object(api,'private_key',side_effect=AssertionError('Real key read forbidden')):
            with RunningServer(api.create_server(settings())) as server:
                self.assertEqual(self.request(server,'/private-transcripts')[0],404)
                for path in ('/','/nodes','/nodes/'):
                    _,_,body=self.request(server,path)
                    self.assertNotIn(b'Private conversations',body)
                    self.assertNotIn(b'/private-transcripts',body)
                status,_,body=self.request(server,'/v1/snapshot?mode=demo')
                self.assertEqual(status,200);api.validate(json.loads(body))
        self.assertEqual(self.calls,[])


class PreviewModeTests(unittest.TestCase):
    def test_explicit_demo_mode_survives_node_frame_and_navigation(self):
        with RunningServer(api.create_server(settings())) as server:
            connection=http.client.HTTPConnection('127.0.0.1',server.server_port,timeout=3)
            connection.request('GET','/nodes/?mode=demo')
            response=connection.getresponse();body=response.read();connection.close()
            self.assertEqual(response.status,200)
            self.assertIn(b'workspace=nodes&amp;mode=demo',body)
            self.assertIn(b'href="/?mode=demo"',body)
            self.assertIn(b'href="/nodes/?mode=demo"',body)
            for query in ('mode=demo&mode=hub','mode=unknown'):
                connection=http.client.HTTPConnection('127.0.0.1',server.server_port,timeout=3)
                connection.request('GET','/nodes/?'+query)
                response=connection.getresponse();response.read();connection.close()
                self.assertEqual(response.status,400)


class StartupTests(unittest.TestCase):
    def test_flag_cannot_enable_a_lan_listener_even_with_a_read_token(self):
        for bind in ('0.0.0.0','192.0.2.10','::','example.local','127.0.0.1.evil.example'):
            args=settings(True,bind);args.read_token_file='/not/read/by/this/test'
            with patch.object(Path,'read_text',side_effect=AssertionError('No credential read allowed')):
                with self.assertRaises(ValueError):api.create_server(args)


if __name__=='__main__':unittest.main()
