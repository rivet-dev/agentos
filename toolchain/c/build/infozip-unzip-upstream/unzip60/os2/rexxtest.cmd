/* Test REXX UnZip API */
call RxFuncAdd 'UZLoadFuncs', 'UNZIP32', 'UZLoadFuncs'
call UZLoadFuncs

parse arg all

say; say 'Demonstrating UZUnZip' UZUnZip(all,'TEST.')
do num=1 to test.0
  say num':'test.num
end

/*** Demonstrate UZFileTree ***/
fname = 'g:\cqc\channel1\12-30.qwk'
say; say 'Demonstrating UZFileTree by displaying all entries in',
          fname
exc.0 = 2
exc.1 = '*.dat'
exc.2 = '*.ndx'
call UZFileTree fname, 'files','','exc'
do num=1 to files.0
  say num':'files.num
end

say; say 'Demonstrating UZUnZipToVar -' UZUnZipToVar(fname,'CONTROL.DAT')


test. = 0
say; say 'Demonstrating UZUnZipToVar -' UZUnZipToVar(fname,'CONTROL.DAT','test.')
SAY "Test =" test.0
do num=1 to test.0
  say num':'test.num
end

test. = 0
say; say 'Demonstrating UZUnZipToStem -' UZUnZipToStem('\SourceCode\cqc\cqcmain.zip','test',"*.rch",,'T')
call recout "test"

say; say 'Demonstrating UZVer -' UZVer()

call UZDropFuncs
exit

recout: PROCEDURE EXPOSE test.
parse arg this
say this "Contains" value(this'.0') "entries"
do num=1 to value(this'.0')
  tval = value(this'.'num)
  say "Got" this'.'num':' tval
  if Right(tval,1) = '/' then
     call recout this'.'left(tval,length(tval)-1)
  else
     say "Contains:" value(this'.tval')
end
return
