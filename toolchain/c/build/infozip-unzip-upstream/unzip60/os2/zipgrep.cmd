/*---------------------------------------------------------------------------

   zipgrep.cmd  (ye olde REXX procedure for OS/2)

   Script to search members of a zipfile for a string or regular expression
   and print the names of any such members (and, optionally, the matching
   text).  The search is case-insensitive by default.

   History:
     original Bourne shell version by Jean-loup Gailly
     modified by Greg Roelofs for Ultrix (no egrep -i) and zipinfo -1
     OS/2 REXX script by Greg Roelofs

   Last modified:  19 Jul 93

  ---------------------------------------------------------------------------*/

PARSE ARG string zipfile members

if (string == '') then do
    say 'usage:  zipgrep search_string zipfile [members...]'
    say '   Displays the names of zipfile members containing a given string,'
    say '   in addition to the matching text.  This procedure requires unzip'
    say '   and egrep in the current path, and it is quite slow....'
    exit 1
end

/* doesn't seem to work...
newq = RXQUEUE("Create",zipgrep_pipe)
oldq = RXQUEUE("Set",newq)
 */

/* flush the queue before starting */
do QUEUED()
    PULL junk
end

/* GRR:  can also add "2>&1" before pipe in following external command */
'@unzip -Z1' zipfile members '| rxqueue'

do while QUEUED() > 0
    PARSE PULL file
    '@unzip -p' zipfile file '| egrep -is' string
    if rc == 0 then do
        SAY file':'
        /* can comment out following line if just want filenames */
        '@unzip -p' zipfile file '| egrep -i' string
    end
end

/*
call RXQUEUE "Delete",newq
call RXQUEUE "Set",oldq
 */

exit 0
