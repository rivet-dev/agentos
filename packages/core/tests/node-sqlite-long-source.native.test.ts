import {spawnSync} from 'node:child_process';
import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';

test('SQLite preparation keeps a multi-megabyte Unicode source prefix without echoing it in metadata',async()=>{
 const source=`const {DatabaseSync}=require('node:sqlite'),db=new DatabaseSync(':memory:');db.exec('CREATE TABLE t(value TEXT)');const value='😀; BEGIN END é'.padEnd(2*1024*1024,'x'),sql="INSERT INTO t VALUES ('"+value+"');",statement=db.prepare(sql+' SELECT 9;');const matches=statement.sourceSQL===sql,characters=statement.sourceSQL.length;statement.run();console.log(JSON.stringify({matches,characters,row:db.prepare('SELECT length(value) AS size,substr(value,1,1) AS first FROM t').get()}));db.close();`;
 const host=spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:10000});expect(host.status,host.stderr).toBe(0);const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'}});let stdout='',stderr='';const child=await vm.process.spawn('node',['-e',source],{timeoutMs:10000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));}finally{await vm?.dispose();await sidecar.dispose();}
},15000);
