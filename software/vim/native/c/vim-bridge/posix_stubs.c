/* posix_stubs.c — stubs for full-OS libc gaps vim references but that the VM
 * does not implement yet. Keep this narrow; the patched sysroot owns libc. */
#include <grp.h>
#include <stddef.h>
struct group *getgrent(void) { return NULL; }
void setgrent(void) {}
void endgrent(void) {}

/* --- process/signal stubs (not used by core editing) --- */
void __SIG_DFL(int s) { (void)s; }
int sigpending(void *set) { (void)set; return 0; }
