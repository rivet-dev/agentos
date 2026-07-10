/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/* swiven.h */

#ifndef __swiven_h
#define __swiven_h

os_error *SWI_OS_FSControl_26(char *source, char *dest, int actionmask);
/* copy */

os_error *SWI_OS_FSControl_27(char *filename, int actionmask);
/* wipe */

os_error *SWI_OS_GBPB_9(char *dirname, void *buf, int *number,
                        int *offset, int size, char *match);
/* read dir */

os_error *SWI_OS_File_1(char *filename, unsigned int loadaddr,
                        unsigned int execaddr, int attrib);
/* write file attributes */

os_error *SWI_OS_File_5(char *filename, int *objtype, unsigned int *loadaddr,
                        unsigned int *execaddr, int *length, int *attrib);
/* read file info */

os_error *SWI_OS_File_6(char *filename);
/* delete */

os_error *SWI_OS_File_7(char *filename, int loadaddr, int execaddr, int size);
/* create an empty file */

os_error *SWI_OS_File_8(char *dirname);
/* create a directory */

os_error *SWI_OS_File_18(char *filename, int filetype);
/* set a file's type */

os_error *SWI_OS_CLI(char *cmd);
/* execute a command */

int SWI_OS_ReadC(void);
/* get a key from the keyboard buffer */

os_error *SWI_OS_ReadVarVal(char *var, char *buf, int len, int *bytesused);
/* reads an OS varibale */

os_error *SWI_OS_FSControl_54(char *buffer, int dir, char *fsname, int *size);
/* reads the path of a specified directory */

os_error *SWI_OS_FSControl_37(char *pathname, char *buffer, int *size);
/* canonicalise path */

os_error *SWI_DDEUtils_Prefix(char *dir);
/* sets the 'prefix' directory */

int SWI_Read_Timezone(void);
/* returns the timezone offset (centiseconds) */

int SWI_MimeMap_Translate(const char *ext);
/* given a filename extn, returns the filetype (or -1 if no match).
 * Uses the MimeMap module */

#endif /* !__swiven_h */
