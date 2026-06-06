#include <unistd.h>
#ifdef chown
#undef chown
#endif
int (*foo)(const char *, uid_t, gid_t) = chown;
int main(void) { return 0; }
