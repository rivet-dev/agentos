/*
  Copyright (c) 1990-2002 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/* riscos.c */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* #define NO_UNZIPH_STUFF */
#define UNZIP_INTERNAL
#include "unzip.h"
#include "riscos.h"

#define MAXEXT 16

char *exts2swap = NULL; /* Extensions to swap (actually, directory names) */

int stat(char *filename,struct stat *res)
{
 int attr;              /* object attributes */
 unsigned int load;     /* load address */
 unsigned int exec;     /* exec address */
 int type;              /* type: 0 not found, 1 file, 2 dir, 3 image */

 if (!res)
   return -1;

 if (SWI_OS_File_5(filename,&type,&load,&exec,(int *)&res->st_size,&attr)!=NULL)
   return -1;

 if (type==0)
   return -1;

 res->st_dev=0;
 res->st_ino=0;
 res->st_nlink=0;
 res->st_uid=1;
 res->st_gid=1;
 res->st_rdev=0;
 res->st_blksize=1024;

 res->st_mode = ((attr & 0001) << 8) | ((attr & 0002) << 6) |
                ((attr & 0020) >> 2) | ((attr & 0040) >> 4);

 switch (type) {
   case 1:                        /* File */
    res->st_mode |= S_IFREG;
    break;
   case 2:                        /* Directory */
    res->st_mode |= S_IFDIR | 0700;
    break;
   case 3:                        /* Image file */
    if (uO.scanimage)
      res->st_mode |= S_IFDIR | 0700;
    else
      res->st_mode |= S_IFREG;
    break;
 }

 if ((((unsigned int) load) >> 20) == 0xfff) {     /* date stamped file */
   register unsigned int t1, t2, tc;

   t1 = (unsigned int) (exec);
   t2 = (unsigned int) (load & 0xff);

   tc = 0x6e996a00U;
   if (t1 < tc)
     t2--;
   t1 -= tc;
   t2 -= 0x33;          /* 00:00:00 Jan. 1 1970 = 0x336e996a00 */

   t1 = (t1 / 100) + (t2 * 42949673U);  /* 0x100000000 / 100 = 42949672.96 */
   t1 -= (t2 / 25);             /* compensate for .04 error */

   res->st_atime = res->st_mtime = res->st_ctime = t1;
 }
 else
   res->st_atime = res->st_mtime = res->st_ctime = 0;

 return 0;
}

#ifndef SFX

DIR *opendir(char *dirname)
{
 DIR *thisdir;
 int type;
 int attr;
 os_error *er;

 thisdir=(DIR *)malloc(sizeof(DIR));
 if (thisdir==NULL)
   return NULL;

 thisdir->dirname=(char *)malloc(strlen(dirname)+1);
 if (thisdir->dirname==NULL) {
   free(thisdir);
   return NULL;
 }

 strcpy(thisdir->dirname,dirname);
 if (thisdir->dirname[strlen(thisdir->dirname)-1]=='.')
   thisdir->dirname[strlen(thisdir->dirname)-1]=0;

 if (er=SWI_OS_File_5(thisdir->dirname,&type,NULL,NULL,NULL,&attr),er!=NULL ||
     type<=1 || (type==3 && !uO.scanimage))
 {
   free(thisdir->dirname);
   free(thisdir);
   return NULL;
 }

 thisdir->buf=malloc(DIR_BUFSIZE);
 if (thisdir->buf==NULL) {
   free(thisdir->dirname);
   free(thisdir);
   return NULL;
 }

 thisdir->size=DIR_BUFSIZE;
 thisdir->offset=0;
 thisdir->read=0;

 return thisdir;
}

struct dirent *readdir(DIR *d)
{
 static struct dirent dent;

 if (d->read==0) {    /* no more objects read in the buffer */
   if (d->offset==-1) {    /* no more objects to read */
     return NULL;
   }

   d->read=255;
   if (SWI_OS_GBPB_9(d->dirname,d->buf,&d->read,&d->offset,DIR_BUFSIZE,NULL)!=NULL)
     return NULL;

   if (d->read==0) {
     d->offset=-1;
     return NULL;
   }
   d->read--;
   d->act=(char *)d->buf;
 }
 else {     /* some object is ready in buffer */
   d->read--;
   d->act=(char *)(d->act+strlen(d->act)+1);
 }

 strcpy(dent.d_name,d->act);
 dent.d_namlen=strlen(dent.d_name);

 /* If we're returning the last item, check if there are any more.
  * If there are, nothing will happen; if not, then d->offset = -1 */
 if (!d->read)
   SWI_OS_GBPB_9(d->dirname,d->buf,&d->read,&d->offset,0,NULL);

 return &dent;
}

void closedir(DIR *d)
{
 if (d->buf!=NULL)
   free(d->buf);
 if (d->dirname!=NULL)
   free(d->dirname);
 free(d);
}

int unlink(f)
char *f;                /* file to delete */
/* Delete the file *f, returning non-zero on failure. */
{
 os_error *er;
 char canon[256];
 int size=255;

 er=SWI_OS_FSControl_37(f,canon,&size);
 if (er==NULL) {
   er=SWI_OS_FSControl_27(canon,0x100);
 }
 else {
   er=SWI_OS_FSControl_27(f,0x100);
 }
 return (int)er;
}

int rmdir(char *d)
{
 int objtype;
 char *s;
 int len;

 len = strlen(d);
 if ((s = malloc(len + 1)) == NULL)
   return -1;

 strcpy(s,d);
 if (s[len-1]=='.')
   s[len-1]=0;

 if (SWI_OS_File_5(s,&objtype,NULL,NULL,NULL,NULL)!=NULL) {
   free(s);
   return -1;
 }
 if (objtype<2 || (!uO.scanimage && objtype==3)) {
/* this is a file or it doesn't exist */
   free(s);
   return -1;
 }
 if (SWI_OS_File_6(s)!=NULL) {
   free(s);
   return -1;
 }
 free(s);
 return 0;
}

#endif /* !SFX */

int chmod(char *file, int mode)
{
/*************** NOT YET IMPLEMENTED!!!!!! ******************/
/* I don't know if this will be needed or not... */
 file=file;
 mode=mode;
 return 0;
}

void setfiletype(char *fname,int ftype)
{
 char str[256];
 sprintf(str,"SetType %s &%3.3X",fname,ftype);
 SWI_OS_CLI(str);
}

void getRISCOSexts(char *envstr)
{
 char *envptr;                               /* value returned by getenv */

 envptr = getenv(envstr);
 if (envptr == NULL || *envptr == 0) return;

 exts2swap=malloc(1+strlen(envptr));
 if (exts2swap == NULL)
   return;

 strcpy(exts2swap, envptr);
}

int checkext(char *suff)
{
 register char *extptr = exts2swap ? exts2swap : "";
 register char *suffptr;
 register int e,s;

 while(*extptr) {
   suffptr=suff;
   e=*extptr; s=*suffptr;
   while (e && e!=':' && s && s!='.' && s!='/' && e==s) {
     e=*++extptr; s=*++suffptr;
   }
   if (e==':') e=0;
   if (s=='.' || s=='/') s=0;
   if (!e && !s) {
     return 1;
   }
   while(*extptr!=':' && *extptr!='\0')    /* skip to next extension */
     extptr++;
   if (*extptr!='\0')
     extptr++;
 }
 return 0;
}

void swapext(char *name, char *exptr)
{
 char ext[MAXEXT];
 register char *p1=exptr+1;
 register char *p2=ext;
 int extchar=*exptr;

 while(*p1 && *p1!='.' && *p1!='/')
   *p2++=*p1++;
 *p2=0;
 p2=exptr-1;
 p1--;
 while(p2 >= name)
   *p1--=*p2--;
 p1=name;
 p2=ext;
 while(*p2)
   *p1++=*p2++;
 *p1=(extchar=='/'?'.':'/');
}

void remove_prefix(void)
{
 SWI_DDEUtils_Prefix(NULL);
}

void set_prefix(void)
{
 char *pref;
 int size=0;

 if (SWI_OS_FSControl_37("@",pref,&size)!=NULL)
   return;

 size=1-size;

 if (pref=malloc(size),pref!=NULL) {
 if (SWI_OS_FSControl_37("@",pref,&size)!=NULL) {
   free(pref);
   return;
 }

 if (SWI_DDEUtils_Prefix(pref)==NULL) {
   atexit(remove_prefix);
 }

 free(pref);
 }
}

#ifdef localtime
#  undef localtime
#endif

#ifdef gmtime
#  undef gmtime
#endif

/* Acorn's implementation of localtime() and gmtime()
 * doesn't consider the timezone offset, so we have to
 * add it before calling the library functions
 */

struct tm *riscos_localtime(const time_t *timer)
{
 time_t localt=*timer;

 localt+=SWI_Read_Timezone()/100;

 return localtime(&localt);
}

struct tm *riscos_gmtime(const time_t *timer)
{
 time_t localt=*timer;

 localt+=SWI_Read_Timezone()/100;

 return gmtime(&localt);
}
