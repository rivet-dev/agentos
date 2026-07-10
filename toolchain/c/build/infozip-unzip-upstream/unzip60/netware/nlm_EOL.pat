diff -rc2 ./fileio.c e:fileio.c
*** ./fileio.c	Sat Dec  4 19:58:26 1999
--- e:fileio.c	Sat Dec  4 20:54:10 1999
***************
*** 85,88 ****
--- 85,91 ----
     (win_fprintf(pG, strm, (extent)len, (char far *)buf) != (int)(len))
  #else /* !WINDLL */
+ #ifdef NLM
+ #  define WriteError(buf,len,strm) nlm_WriteError(buf, (extent)(len), strm)
+ #else /* !NLM */
  #  ifdef USE_FWRITE
  #    define WriteError(buf,len,strm) \
***************
*** 92,95 ****
--- 95,99 ----
       ((extent)write(fileno(strm),(char *)(buf),(extent)(len)) != (extent)(len))
  #  endif
+ #endif /* ?NLM */
  #endif /* ?WINDLL */
  
diff -rc2 ./netware/nlmcfg.h e:netware/nlmcfg.h
*** ./netware/nlmcfg.h	Sat Dec  4 20:39:20 1999
--- e:netware/nlmcfg.h	Sat Dec  4 21:20:36 1999
***************
*** 21,25 ****
  #  define lenEOL          2
  #  define PutNativeEOL  {*q++ = native(CR); *q++ = native(LF);}
- #  define USE_FWRITE    /* write() fails to support textmode output */
  #  if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))
  #    define TIMESTAMP
--- 21,24 ----
***************
*** 30,32 ****
--- 29,32 ----
     void InitUnZipConsole OF((void));
     int screenlines       OF((void));
+    int nlm_WriteError    OF((uch *buf, extent len, FILE *strm));
  #endif /* NLM */
diff -rc2 ./netware/netware.c e:netware/netware.c
*** ./netware/netware.c	Sat Dec  4 21:11:52 1999
--- e:netware/netware.c	Sat Dec  4 21:28:38 1999
***************
*** 22,25 ****
--- 22,26 ----
               version()
               screenlines()
+              nlm_WriteError()
  
    ---------------------------------------------------------------------------*/
***************
*** 821,822 ****
--- 822,850 ----
  
  #endif /* MORE */
+ 
+ 
+ /*******************************/
+ /*  Function nlm_WriteError()  */
+ /*******************************/
+ 
+ int nlm_WriteError(buf, len, strm)
+     uch *buf;
+     extent len;
+     FILE *strm;
+ {
+     /* The write() implementation in the Novell C RTL lacks support of
+        text-mode streams (fails to translate '\n' into "CR-LF" when
+        writing to text-mode channels like the console).
+        In contrast, fwrite() takes into account when an output stream
+        was opened in text-mode, but fails to handle output of large
+        buffers correctly.
+        So, we have to use Unix I/O style write() when emitting data
+        to "regular" files but switch over to stdio's fwrite() when
+        writing to the console streams.
+      */
+     if ((strm == stdout)) || (file == stderr)) {
+          return ((extent)fwrite((char *)buf, 1, len, strm) != len);
+     } else {
+          return ((extent)write(fileno(strm), (char *)buf, len) != len);
+     }
+ } /* end function nlm_WriteError() */
