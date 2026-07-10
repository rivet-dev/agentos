/* $Copyright: $
 * Copyright (c) 1996 - 2026 by Steve Baker (steve.baker.llc@gmail.com)
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program; if not, write to the Free Software
 * Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
 */
#include "tree.h"

extern struct Flags flag;
extern const char *charset;
extern const mode_t ifmt[];
extern const char *ftype[];

extern FILE *outfile;
extern char *_nl;

/*
<tree>
  <directory name="name" mode=0777 size=### user="user" group="group" inode=### dev=### time="00:00 00-00-0000">
    <link name="name" target="name" ...>
      ... if link is followed, otherwise this is empty.
    </link>
    <file name="name" mode=0777 size=### user="user" group="group" inode=### dev=### time="00:00 00-00-0000"></file>
    <socket name="" ...><error>some error</error></socket>
    <block name="" ...></block>
    <char name="" ...></char>
    <fifo name="" ...></fifo>
    <door name="" ...></door>
    <port name="" ...></port>
    ...
  </directory>
  <report>
    <size>#</size>
    <files>#</files>
    <directories>#</directories>
  </report>
</tree>
*/

void xml_indent(int maxlevel)
{
  char *spaces[] = {"    ", "   ", "  ", " ", ""};
  int clvl = flag.compress_indent + (flag.remove_space? 1 : 0);
  int i;

  if (flag.noindent) return;
  fprintf(outfile, "%s", spaces[clvl]);
  for(i=0; i<maxlevel; i++)
    fprintf(outfile, "%s", spaces[clvl]);
}

void xml_fillinfo(struct _info *ent)
{
  #ifdef __USE_FILE_OFFSET64
  if (flag.inode) fprintf(outfile," inode=\"%lld\"",(long long)ent->inode);
  #else
  if (flag.inode) fprintf(outfile," inode=\"%ld\"",(long int)ent->inode);
  #endif
  if (flag.dev) fprintf(outfile, " dev=\"%d\"", (int)ent->dev);
  #ifdef __EMX__
  if (flag.p) fprintf(outfile, " mode=\"%04o\" prot=\"%s\"",ent->attr, prot(ent->attr));
  #else
  if (flag.p) fprintf(outfile, " mode=\"%04o\" prot=\"%s\"", ent->mode & (S_IRWXU|S_IRWXG|S_IRWXO|S_ISUID|S_ISGID|S_ISVTX), prot(ent->mode));
  #endif
  if (flag.u) fprintf(outfile, " user=\"%s\"", uidtoname(ent->uid));
  if (flag.g) fprintf(outfile, " group=\"%s\"", gidtoname(ent->gid));
  if (flag.s) fprintf(outfile, " size=\"%lld\"", (long long int)(ent->size));
  if (flag.D) fprintf(outfile, " time=\"%s\"", do_date(flag.c? ent->ctime : ent->mtime));
}

void xml_intro(void)
{
  extern char *_nl;

  fprintf(outfile,"<?xml version=\"1.0\"");
  if (charset) fprintf(outfile," encoding=\"%s\"",charset);
  fprintf(outfile,"?>%s<tree>%s",_nl,_nl);
}

void xml_outtro(void)
{
  fprintf(outfile,"</tree>%s", _nl);
}

int xml_printinfo(char *dirname, struct _info *file, int level)
{
  UNUSED(dirname);

  mode_t mt;
  int t;

  if (!flag.noindent) xml_indent(level);

  if (file != NULL) {
    if (file->lnk) mt = file->mode & S_IFMT;
    else mt = file->mode & S_IFMT;
  } else mt = 0;

  for(t=0;ifmt[t];t++)
    if (ifmt[t] == mt) break;
  if (file) file->tag = ftype[t];
  fprintf(outfile,"<%s", ftype[t]);

  return 0;
}

int xml_printfile(char *dirname, char *filename, struct _info *file, int descend)
{
  UNUSED(dirname);UNUSED(descend);

  int i;

  fprintf(outfile, " name=\"");
  html_encode(outfile, filename);
  fputc('"',outfile);

  if (file) {
    if (file->comment) {
      fprintf(outfile, " info=\"");
      for(i=0; file->comment[i]; i++) {
        html_encode(outfile, file->comment[i]);
        if (file->comment[i+1]) fprintf(outfile, "%s", _nl);
      }
      fputc('"', outfile);
    }
    if (file->lnk) {
      fprintf(outfile, " target=\"");
      html_encode(outfile,file->lnk);
      fputc('"',outfile);
    }
    xml_fillinfo(file);
  }
  fputc('>',outfile);

  return 1;
}

int xml_error(char *error)
{
  fprintf(outfile,"<error>%s</error>", error);

  return 0;
}

void xml_newline(struct _info *file, int level, int postdir, int needcomma)
{
  UNUSED(file);UNUSED(level);UNUSED(needcomma);

  if (postdir >= 0) fprintf(outfile, "%s", _nl);
}

void xml_close(struct _info *file, int level, int needcomma)
{
  UNUSED(needcomma);

  if (!flag.noindent && level >= 0) xml_indent(level);
  fprintf(outfile,"</%s>%s", file? file->tag : "unknown", flag.noindent? "" : _nl);
}


void xml_report(struct totals tot)
{
  extern char *_nl;

  xml_indent(0); fprintf(outfile,"<report>%s", _nl);
  if (flag.du) {
    xml_indent(1); fprintf(outfile,"<size>%lld</size>%s", (long long int)tot.size, _nl);
  }
  xml_indent(1); fprintf(outfile,"<directories>%ld</directories>%s", tot.dirs, _nl);
  if (!flag.d) {
    xml_indent(1); fprintf(outfile,"<files>%ld</files>%s", tot.files, _nl);
  }
  xml_indent(0); fprintf(outfile,"</report>%s", _nl);
}
