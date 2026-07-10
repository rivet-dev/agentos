/**** REXX  ********   ZIP2EXE.CMD  **************  01/04/96 *********\
|**  This exec will prepend the Info Zip unzipsfx.exe file to an    **|
|**  existing ZIP file to create a self extracting zip.             **|
|**                                                                 **|
|**  The exec requires 1 argument, the name of the zip file to be   **|
|**  acted upon.                                                    **|
|**                                                                 **|
|**  Put this exec into the path that contains your Info Zip        **|
|**  executables.                                                   **|
|**                                                                 **|
\*********************************************************************/
rc = 0
/**  Start Argument processing  ** End Initialization               **/
PARSE UPPER ARG zip_file
IF zip_file = ""
THEN
  DO
    SAY "You must specify the name of the file to be processed"
    SAY "Please try again"
    rc = 9
    SIGNAL FINI
  END
IF POS(".ZIP",zip_file) = 0
THEN
  DO
    sfx_file = zip_file||".EXE"
    zip_file = zip_file||".ZIP"
  END
ELSE
    sfx_file = SUBSTR(zip_file,1,LASTPOS(".",zip_file))||"EXE"
zip_file = STREAM(zip_file,"C","QUERY EXISTS")
IF zip_file = ""
THEN
  DO
    SAY "The file "||ARG(1)||" Does not exist"
    SAY "Processing terminated"
    rc = 9
    SIGNAL FINI
  END
/**  Start unzipsfx location    ** End Argument processing          **/
PARSE UPPER SOURCE . . command_file
unzipsfx = SUBSTR(command_file,1,LASTPOS("\",command_file))||,
          "UNZIPSFX.EXE"
IF STREAM(unzipsfx,"C","QUERY EXISTS") = ""
THEN
  DO
    SAY "We are unable to locate the UNZIPSFX.EXE source"
    SAY "Ensure that the ZIP2EXE command is in the directory",
        "which contains UNZIPSFX.EXE"
    rc = 9
    SIGNAL FINI
  END
/**  Execute the command        ** End Argument processing          **/
ADDRESS CMD "@COPY /b "||unzipsfx||"+"||zip_file,
            sfx_file||" > NUL"
IF rc = 0
THEN
  SAY sfx_file||" successfully created"
ELSE
  SAY sfx_file||" creation failed"
FINI:
  EXIT  rc
