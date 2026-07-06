#ifndef _GRP_H
#define _GRP_H
#ifdef __cplusplus
extern "C" {
#endif
#define __NEED_gid_t
#define __NEED_size_t
#include <bits/alltypes.h>
struct group {
	char *gr_name;
	char *gr_passwd;
	gid_t gr_gid;
	char **gr_mem;
};
struct group *getgrgid(gid_t);
struct group *getgrnam(const char *);
struct group *getgrent(void);
void setgrent(void);
void endgrent(void);
#ifdef __cplusplus
}
#endif
#endif
