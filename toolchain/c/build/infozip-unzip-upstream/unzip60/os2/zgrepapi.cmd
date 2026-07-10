/*---------------------------------------------------------------------------

   zapigrep.cmd  (ye olde REXX procedure for OS/2)

   Script to search members of a zipfile for a string and print the names of
   any such members (and, optionally, the matching text).

   This is the zipgrep.cmd program completely rewritten to take advantage
   of the REXX API.  Among the improvements are:
        egrep no longer needed.  All work done by API and this script.
	Added option switches.

   Be aware, however, that this script does not support regular expressions
   as zipgrep does.

  ---------------------------------------------------------------------------*/

PARSE ARG args

nocase = 0
word = 0
text = 0

DO WHILE Left(args,1) == '-' | Left(args,1) == '/'
   PARSE VAR args option args
   option = Translate(SubStr(option,2))
   DO WHILE option \= ''
      oneopt = Left(option,1)
      option = SubStr(option,2)
      SELECT
       WHEN oneopt == 'W' THEN
	  word = 1
       WHEN oneopt == 'I' THEN
	  nocase = 1
       WHEN oneopt == 'T' THEN
	  text = 1
       OTHERWISE NOP
      END
   END
END

PARSE VAR args string zipfile members

IF (string == '') THEN DO
   SAY 'usage:  zipgrep [-i][-t][-w] search_string zipfile [members...]'
   SAY '   Displays the names of zipfile members containing a given string.'
   SAY '   By default it displays filenames only of members containing an'
   SAY '   exact match of the string.  Options are:'"0a"x
   SAY '	-i  Ignore case'
   SAY '	-t  Display matching lines'
   SAY '	-w  Match words only'
   EXIT 1
END
string = Strip(string,'b','"')
IF nocase THEN string = Translate(string)

CALL RxFuncAdd 'UZLoadFuncs', 'UNZIP32', 'UZLoadFuncs'
CALL UZLoadFuncs

CALL UZUnZipToStem zipfile, 'file.', members

DO i = 1 TO file.0
   ptr = file.i
   file = file.ptr
   IF nocase THEN file = Translate(file)
   IF word THEN DO
      wp = WordPos(string,file)
      IF wp>0 THEN
	 scan = WordIndex(file,wp)
      ELSE
         scan = 0
   END
   ELSE
      scan = Pos(string,file)
   IF scan \= 0 THEN DO
      SAY file.i':'
      IF text THEN DO
	 DO WHILE scan > 0
	    from = LastPos('0a'x,file,scan)+1
	    to = Pos('0a'x,file,scan)
	    IF to = 0 THEN to = Length(file.ptr)
	    oneline = Strip(SubStr(file.ptr,from,to-from),'T','0a'x)
	    SAY Strip(oneline,'T','0d'x)
	    IF word THEN DO
	       wp = WordPos(string,file,wp+1)
	       IF wp>0 THEN
		  scan = WordIndex(file,wp)
	       ELSE
		  scan = 0
	    END
	    ELSE
	       scan = Pos(string,file,scan+1)
	 END
      END
   END
END

EXIT 0
