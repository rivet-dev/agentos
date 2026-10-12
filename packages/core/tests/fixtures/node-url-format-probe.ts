// Same finite program runs in Node and the guest. Keep the application RED unchanged.
export const urlFormatProbe = String.raw`
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const cjs = require('node:url');
const esm = await import('node:url');
const rows = [];
const inputs = [
  'postgres://admin:secret@db.example.com:5432/app?sslmode=require#primary',
  'https://user:p%40ss@xn--bcher-kva.example:8443/a%20b?q=%23#part',
  'https://[::1]:8443/a?b#c',
  'file:///tmp/a%20b?#',
  'mailto:user@example.com?subject=hello#part',
  'custom://user:pass@xn--bcher-kva.example:123/path?#',
  'https://example.com/path?#',
];
const options = [undefined, {}, {auth:false}, {fragment:false}, {search:false},
  {unicode:true}, {auth:false,fragment:false,search:false,unicode:true},
  {auth:0,fragment:null,search:'',unicode:1}];
function capture(fn) {
  try { return {value:fn()}; }
  catch(error) { return {error:{name:error.name,code:error.code ?? null}}; }
}
for (const [route, api] of [['CJS',cjs],['ESM',esm]]) {
  for (const input of inputs) {
    for (let index=0;index<options.length;index++) {
      const value = new URL(input), before=value.href;
      rows.push({route,kind:'whatwg',input,option:index,...capture(()=>api.format(value,options[index])),unchanged:value.href===before});
    }
  }
  for (const [name,option] of [['undefined',undefined],['null',null],['false',false],['zero',0],['NaN',NaN],['empty',''],['true',true],['one',1],['string','x'],['array',[]],['function',()=>{}]]) {
    rows.push({route,kind:'options',name,...capture(()=>api.format(new URL(inputs[0]),option))});
  }
  for (const [name,value] of [['string',inputs[0]],['parsed',api.parse(inputs[0])],['object',{protocol:'https:',slashes:true,auth:'user:pass',hostname:'example.com',pathname:'/path',search:'?q=1',hash:'#part'}]]) {
    rows.push({route,kind:'legacy',name,...capture(()=>api.format(value,{auth:false,fragment:false,search:false}))});
  }
  const order=[], option={};
  for(const key of ['auth','search','unicode','fragment']) Object.defineProperty(option,key,{get(){order.push(key);return key==='unicode';}});
  rows.push({route,kind:'getter-order',...capture(()=>api.format(new URL(inputs[0]),option)),order});
  const value=new URL(inputs[0]);
  Object.defineProperty(value,'href',{value:inputs[1]});
  rows.push({route,kind:'own-href',...capture(()=>api.format(value,{unicode:true}))});
}
console.log(JSON.stringify({sameExport:cjs.format===esm.format,rows}));
`;
