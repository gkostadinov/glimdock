import {DatabaseSync} from 'node:sqlite';
import {readFileSync,readdirSync} from 'node:fs';

// Exercise the production migrations and SQL using SQLite, with the same
// asynchronous statement shape used by the Sites D1 binding.
export function fixtureDB(){
 const connection=new DatabaseSync(':memory:');
 for(const file of readdirSync(new URL('../../drizzle/',import.meta.url)).filter(x=>/^\d{4}.*\.sql$/.test(x)).sort())connection.exec(readFileSync(new URL('../../drizzle/'+file,import.meta.url),'utf8'));
 return {connection,prepare(sql){const statement=connection.prepare(sql);let values=[];return {bind(...x){values=x;return this;},async first(){return statement.get(...values)||null;},async all(){return {results:statement.all(...values)};},async run(){const result=statement.run(...values);return {meta:{changes:Number(result.changes)}};}};}};
}
