/*
  Copyright (c) 1990-2005 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  vmmvs.c (for both VM/CMS and MVS)

  Contains:  vmmvs_open_infile()
             open_outfile()
             close_outfile()
             close_infile()
             getVMMVSexfield()
             do_wild()
             mapattr()
             mapname()
             checkdir()
             check_for_newer()
             stat()
             version()

  ---------------------------------------------------------------------------*/


#define __VMMVS_C       /* identifies this source module */
#define UNZIP_INTERNAL
#include "unzip.h"


/********************************/
/* Function vmmvs_open_infile() */
/********************************/

FILE *vmmvs_open_infile(__G)
   __GDEF
{
   FILE *fzip;

   G.tempfn = NULL;

   fzip = fopen(G.zipfn, FOPR);

#if 0
   /* Let's try it without the convert for a while -- RG Hartwig */

   if ((fzip = fopen(G.zipfn,"rb,recfm=fb")) == NULL) {
      size_t cnt;
      char *buf;
      FILE *in, *out;

      if ((buf = (char *)malloc(32768)) == NULL) return NULL;
      if ((G.tempfn = tmpnam(NULL)) == NULL) return NULL;
      if ((in = fopen(G.zipfn,"rb")) != NULL &&
          (out = fopen(G.tempfn,"wb,recfm=fb,lrecl=1")) != NULL) {
         Trace((stdout,"Converting ZIP file to fixed record format...\n"));
         while (!feof(in)) {
            cnt = fread(buf,1,32768,in);
            if (cnt) fwrite(buf,1,cnt,out);
         }
      }
      else {
         free(buf);
         fclose(out);
         fclose(in);
         return NULL;
      }
      free(buf);
      fclose(out);
      fclose(in);

      fzip = fopen(G.tempfn,"rb,recfm=fb");
      if (fzip == NULL) return NULL;

      /* Update the G.ziplen value since it might have changed after
         the reformatting copy. */
      fseek(fzip,0L,SEEK_SET);
      fseek(fzip,0L,SEEK_END);
      G.ziplen = ftell(fzip);
   }

#endif

   return fzip;
}


/***************************/
/* Function open_outfile() */
/***************************/

int open_outfile(__G)           /* return 1 if fail */
    __GDEF
{
    char type[100];
    char *mode = NULL;
#ifdef MVS
    /* Check if the output file already exists and do not overwrite its DCB */
    char basefilename[PATH_MAX], *p;
    FILE *exists;

    /* Get the base file name, without any member name */
    strcpy(basefilename, G.filename);
    if ((p = strchr(basefilename, '(')) != NULL) {
       if (basefilename[0] == '\'')
          *p++ = '\'';
       *p = '\0';
    }
    exists = fopen(basefilename, FOPR);
    if (exists) {
       if (G.pInfo->textmode)
           mode = FOPWTE;       /* Text file, existing */
       else
           mode = FOPWE;        /* Binary file, existing */
       fclose(exists);
    }
    else   /* continued on next line */
#endif /* MVS */
    if (G.pInfo->textmode) {
        if (mode == NULL)
           mode = FOPWT;
    } else if (G.lrec.extra_field_length > 0 && G.extra_field != NULL) {
        unsigned lef_len = (unsigned)(G.lrec.extra_field_length);
        uch *lef_buf = G.extra_field;

        while (lef_len > EB_HEADSIZE) {
            unsigned eb_id = makeword(&lef_buf[EB_ID]);
            unsigned eb_dlen = makeword(&lef_buf[EB_LEN]);

            if (eb_dlen > (lef_len - EB_HEADSIZE)) {
                /* Discovered some extra field inconsistency! */
                TTrace((stderr,
                        "open_outfile: block length %u > rest lef_size %u\n",
                        eb_dlen, lef_len - EB_HEADSIZE));
                break;
            }

            if ((eb_id == EF_VMCMS || eb_id == EF_MVS) &&
                (getVMMVSexfield(type, lef_buf, eb_dlen) > 0)) {
                mode = type;
                break;
            }

            /* Skip this extra field block */
            lef_buf += (eb_dlen + EB_HEADSIZE);
            lef_len -= (eb_dlen + EB_HEADSIZE);
        }
    }
    if (mode == NULL) mode = FOPW;

    Trace((stderr, "Output file='%s' opening with '%s'\n", G.filename, mode));
    if ((G.outfile = fopen(G.filename, mode)) == NULL) {
        Info(slide, 0x401, ((char *)slide, "\nerror:  cannot create %s\n",
             FnFilter1(G.filename)));
        Trace((stderr, "error %d: '%s'\n", errno, strerror(errno)));
        return 1;
    }
    return 0;
} /* end function open_outfile() */


/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)
   __GDEF
{
   fclose(G.outfile);
} /* end function close_outfile() */


/***************************/
/* Function close_infile() */
/***************************/

void close_infile(__G)
   __GDEF
{
   fclose(G.zipfd);

   /* If we're working from a temp file, erase it now */
   if (G.tempfn)
      remove(G.tempfn);

} /* end function close_infile() */



/******************************/
/* Function getVMMVSexfield() */
/******************************/

extent getVMMVSexfield(type, ef_block, datalen)
    char *type;
    uch *ef_block;
    unsigned datalen;
{
    fldata_t *fdata = (fldata_t *) &ef_block[4];

    if (datalen < sizeof(fldata_t))
        return 0;

    strcpy(type, "w");
    strcat(type,  fdata->__openmode == __TEXT   ? ""
                 :fdata->__openmode == __BINARY ? "b"
                 :fdata->__openmode == __RECORD ? "b,type=record"
                 :                                "");
    strcat(type, ",recfm=");
    strcat(type,  fdata->__recfmF? "F"
                 :fdata->__recfmV? "V"
                 :fdata->__recfmU? "U"
                 :                 "?");
    if (fdata->__recfmBlk) strcat(type, "B");
    if (fdata->__recfmS)   strcat(type, "S");
    if (fdata->__recfmASA) strcat(type, "A");
    if (fdata->__recfmM)   strcat(type, "M");
    sprintf(type+strlen(type), ",lrecl=%ld", fdata->__recfmV
                                              ? fdata->__maxreclen+4
                                              : fdata->__maxreclen);
#ifdef VM_CMS
    /* For CMS, use blocksize for FB files only */
    if (fdata->__recfmBlk)
       sprintf(type+strlen(type), ",blksize=%ld", fdata->__blksize);
#else
    /* For MVS, always use blocksize */
    sprintf(type+strlen(type), ",blksize=%ld", fdata->__blksize);
#endif

    return strlen(type);
} /* end function getVMMVSexfield() */



#ifndef SFX

/**********************/
/* Function do_wild() */   /* for porting: dir separator; match(ignore_case) */
/**********************/

char *do_wild(__G__ wld)
    __GDEF
    ZCONST char *wld;      /* only used first time on a given dir */
{
    static int First = 0;
    static char filename[256];

    if (First == 0) {
       First = 1;
       strncpy(filename, wld, sizeof(filename));
       filename[sizeof(filename)-1] = '\0';
       return filename;
    }
    else
       return (char *)NULL;

} /* end function do_wild() */

#endif /* !SFX */



/************************/
/*  Function mapattr()  */
/************************/

int mapattr(__G)
     __GDEF
{
    return 0;
}

/************************/
/*  Function mapname()  */
/************************/

int mapname(__G__ renamed)
    __GDEF
    int renamed;
/*
 * returns:
 *  MPN_OK          - no problem detected
 *  MPN_INF_TRUNC   - caution (truncated filename)
 *  MPN_INF_SKIP    - info "skip entry" (dir doesn't exist)
 *  MPN_ERR_SKIP    - error -> skip entry
 *  MPN_ERR_TOOLONG - error -> path is too long
 *  MPN_NOMEM       - error (memory allocation failed) -> skip entry
 *  [also MPN_VOL_LABEL, MPN_CREATED_DIR]
 */
{
    char newname[FILNAMSIZ], *lbar;
#ifdef MVS
    char *pmember;
#endif
    int name_changed = MPN_OK;

    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* can't set disk volume labels in CMS_MVS */

#ifdef MVS
    /* Remove bad characters for MVS from the filename */
    while ((lbar = strpbrk(G.filename, "_+-")) != NULL) {
       /* Must use memmove() here because data overlaps.  */
       /* strcpy() gives undefined behavior in this case. */
       memmove(lbar, lbar+1, strlen(lbar));
       name_changed = MPN_INF_TRUNC;
    }
#endif

    /* Remove bad characters for MVS/CMS from the filename */
    while ((lbar = strpbrk(G.filename, "()")) != NULL) {
       memmove(lbar, lbar+1, strlen(lbar));
       name_changed = MPN_INF_TRUNC;
    }

#ifdef VM_CMS
    if ((lbar = strrchr(G.filename, '/')) != NULL) {
        strcpy(newname, lbar+1);
        Trace((stderr, "File '%s' renamed to '%s'\n", G.filename, newname));
        strcpy(G.filename, newname);
        name_changed = MPN_INF_TRUNC;
    }
#else /* MVS */
    if ((pmember = strrchr(G.filename, '/')) == NULL)
        pmember = G.filename;
    else
        pmember++;

    /* search for extension in file name */
    if ((lbar = strrchr(pmember, '.')) != NULL) {
        *lbar++ = '\0';
        strcpy(newname, pmember);
        strcpy(pmember, lbar);
        strcat(pmember, "(");
        strcat(pmember, newname);
        strcat(pmember, ")");
    }

    /* Remove all 'internal' dots '.', to prevent false consideration as
     * MVS path delimiters! */
    while ((lbar = strrchr(G.filename, '.')) != NULL) {
        memmove(lbar, lbar+1, strlen(lbar));
        name_changed = MPN_INF_TRUNC;
    }

    /* Finally, convert path delimiters from internal '/' to external '.' */
    while ((lbar = strchr(G.filename, '/')) != NULL)
        *lbar = '.';
#endif /* ?VM_CMS */

#ifndef MVS
    if ((lbar = strchr(G.filename, '.')) == NULL) {
        printf("WARNING: file '%s' has no extension - renamed to '%s.NONAME'\n"\
              ,G.filename, G.filename);
       strcat(G.filename, ".NONAME");
       name_changed = MPN_INF_TRUNC;
    }
#endif
    checkdir(__G__ G.filename, GETPATH);

    return name_changed;

} /* end function mapname() */


int checkdir(__G__ pathcomp, flag)
    __GDEF
    char *pathcomp;
    int flag;
/*
 * returns:
 *  MPN_OK          - no problem detected
 *  MPN_INF_TRUNC   - (on APPEND_NAME) truncated filename
 *  MPN_INF_SKIP    - path doesn't exist, not allowed to create
 *  MPN_ERR_SKIP    - path doesn't exist, tried to create and failed; or path
 *                    exists and is not a directory, but is supposed to be
 *  MPN_ERR_TOOLONG - path is too long
 *  MPN_NOMEM       - can't allocate memory for filename buffers
 */
{
    static int rootlen = 0;     /* length of rootpath */
    static char *rootpath;      /* user's "extract-to" directory */

#   define FN_MASK   7
#   define FUNCTION  (flag & FN_MASK)


/*---------------------------------------------------------------------------
    ROOT:  if appropriate, store the path in rootpath and create it if neces-
    sary; else assume it's a zipfile member and return.  This path segment
    gets used in extracting all members from every zipfile specified on the
    command line.  Note that under OS/2 and MS-DOS, if a candidate extract-to
    directory specification includes a drive letter (leading "x:"), it is
    treated just as if it had a trailing '/'--that is, one directory level
    will be created if the path doesn't exist, unless this is otherwise pro-
    hibited (e.g., freshening).
  ---------------------------------------------------------------------------*/

#if (!defined(SFX) || defined(SFX_EXDIR))
    if (FUNCTION == ROOT) {
        Trace((stderr, "initializing root path to [%s]\n",
          FnFilter1(pathcomp)));
        if (pathcomp == (char *)NULL) {
            rootlen = 0;
        }
        else if ((rootlen = strlen(pathcomp)) > 0) {
            if ((rootpath = (char *)malloc(rootlen+1)) == NULL) {
                rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(rootpath, pathcomp);
            Trace((stderr, "rootpath now = [%s]\n", rootpath));
        }
        return MPN_OK;
    }
#endif /* !SFX || SFX_EXDIR */

/*---------------------------------------------------------------------------
    GETPATH:  copy full path to the string pointed at by pathcomp, and free
    buildpath.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == GETPATH) {
        if (rootlen > 0) {
#ifdef VM_CMS                     /* put the exdir after the filename */
           strcat(pathcomp, ".");       /* used as minidisk to be save on  */
           strcat(pathcomp, rootpath);
#else /* MVS */
           char newfilename[PATH_MAX];
           char *start_fname;
           int quoted = 0;

           strcpy(newfilename, rootpath);
           if (newfilename[0] == '\'') {
              quoted = strlen(newfilename) - 1;
              if (newfilename[quoted] == '\'')
                 newfilename[quoted] = '\0';
              else
                 quoted = 0;
           }
           if (strchr(pathcomp, '(') == NULL) {
              if ((start_fname = strrchr(pathcomp, '.')) == NULL) {
                 start_fname = pathcomp;
              }
              else {
                 *start_fname++ = '\0';
                 strcat(newfilename, ".");
                 strcat(newfilename, pathcomp);
              }
              strcat(newfilename, "(");
              strcat(newfilename, start_fname);
              strcat(newfilename, ")");
           }
           else {
              strcat(newfilename, ".");
              strcat(newfilename, pathcomp);
           }
           if (quoted)
              strcat(newfilename, "'");
           Trace((stdout, "new dataset : %s\n", newfilename));
           strcpy(pathcomp, newfilename);
#endif /* ?VM_CMS */
        }
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    END:  free rootpath, immediately prior to program exit.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == END) {
        Trace((stderr, "freeing rootpath\n"));
        if (rootlen > 0) {
            free(rootpath);
            rootlen = 0;
        }
        return MPN_OK;
    }

    return MPN_INVALID; /* should never reach */

} /* end function checkdir() */




/******************************/
/* Function check_for_newer() */  /* used for overwriting/freshening/updating */
/******************************/

int check_for_newer(__G__ filename)  /* return 1 if existing file is newer */
    __GDEF                           /*  or equal; 0 if older; -1 if doesn't */
    char *filename;                  /*  exist yet */
{
    FILE *stream;

    if ((stream = fopen(filename, FOPR)) != NULL) {
       fclose(stream);
       /* File exists, assume it is "newer" than archive entry. */
       return EXISTS_AND_NEWER;
    }
    /* File does not exist. */
    return DOES_NOT_EXIST;
} /* end function check_for_newer() */


/*********************/
/*  Function stat()  */
/*********************/

int stat(const char *path, struct stat *buf)
{
   FILE *fp;
   char fname[PATH_MAX];
   time_t ltime;

   if ((fp = fopen(path, FOPR)) != NULL) {
      fldata_t fdata;
      if (fldata( fp, fname, &fdata ) == 0) {
         buf->st_dev  = fdata.__device;
         buf->st_mode = *(short *)(&fdata);
      }

      /* Determine file size by seeking to EOF */
      fseek(fp,0L,SEEK_END);
      buf->st_size = ftell(fp);
      fclose(fp);

      /* set time fields in stat buf to current time. */
      time(&ltime);
      buf->st_atime =
      buf->st_mtime =
      buf->st_ctime = ltime;

      /* File exists, return success */
      return 0;
   }
   return 1;
}



#ifdef STAND_ALONE
/***************************/
/*  Function main_vmmvs()  */
/***************************/

/* This function is called as main() to parse arguments                */
/* into argc and argv.  This is required for stand-alone               */
/* execution.  This calls the "real" main() when done.                 */

int MAIN_VMMVS(void)
   {
    int  argc=0;
    char *argv[50];

    int  iArgLen;
    char argstr[256];
    char **pEPLIST, *pCmdStart, *pArgStart, *pArgEnd;

   /* Get address of extended parameter list from S/370 Register 0 */
   pEPLIST = (char **)__xregs(0);

   /* Null-terminate the argument string */
   pCmdStart = *(pEPLIST+0);
   pArgStart = *(pEPLIST+1);
   pArgEnd   = *(pEPLIST+2);
   iArgLen   = pArgEnd - pCmdStart + 1;

   /* Make a copy of the command string */
   memcpy(argstr, pCmdStart, iArgLen);
   argstr[iArgLen] = '\0';  /* Null-terminate */

   /* Store first token (cmd) */
   argv[argc++] = strtok(argstr, " ");

   /* Store the rest (args) */
   while (argv[argc-1])
      argv[argc++] = strtok(NULL, " ");
   argc--;  /* Back off last NULL entry */

   /* Call "real" main() function */
   return MAIN(argc, argv);
}
#endif  /* STAND_ALONE */



#ifndef SFX

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
    int len;
    char liblvlmsg [50+1];
    char *compiler = "?";
    char *platform = "?";
    char complevel[64];

    /* Map the runtime library level information */
    union {
       unsigned int iVRM;
       struct {
          unsigned int pd:4;    /* Product designation */
          unsigned int vv:4;    /* Version             */
          unsigned int rr:8;    /* Release             */
          unsigned int mm:16;   /* Modification level  */
       } xVRM;
    } VRM;


    /* Break down the runtime library level */
    VRM.iVRM = __librel();
    sprintf(liblvlmsg, "Using runtime library level %s V%dR%dM%d",
            (VRM.xVRM.pd==1 ? "LE" : "CE"),
            VRM.xVRM.vv, VRM.xVRM.rr, VRM.xVRM.mm);
    /* Note:  LE = Language Environment, CE = Common Env. (C/370). */
    /* This refers ONLY to the current runtimes, not the compiler. */


#ifdef VM_CMS
    platform = "VM/CMS";
    #ifdef __IBMC__
       compiler = "IBM C";
    #else
       compiler  = "C/370";
    #endif
#endif

#ifdef MVS
    platform = "MVS";
    #ifdef __IBMC__
       compiler = "IBM C/C++";
    #else
       compiler = "C/370";
    #endif
#endif

#ifdef __COMPILER_VER__
    VRM.iVRM = __COMPILER_VER__;
    sprintf(complevel," V%dR%dM%d",
            VRM.xVRM.vv, VRM.xVRM.rr, VRM.xVRM.mm);
#else
#ifdef __IBMC__
    sprintf(complevel," V%dR%d", __IBMC__ / 100, (__IBMC__ % 100)/10);
#else
    complevel[0] = '\0';
#endif
#endif


    /* Output is in the form "Compiled with %s%s for %s%s%s%s." */
    len = sprintf((char *)slide, LoadFarString(CompiledWith),

    /* Add compiler name and level */
    compiler, complevel,

    /* Add compile environment */
    platform,

    /* Add timestamp */
#ifdef __DATE__
      " on " __DATE__
#ifdef __TIME__
      " at " __TIME__
#endif
#endif
      ".\n", "",
      liblvlmsg
    );

    (*G.message)((zvoid *)&G, slide, (ulg)len, 0);

} /* end function version() */

#endif /* !SFX */
