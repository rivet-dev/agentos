#include <grp.h>
#ifdef getgrnam_r
#undef getgrnam_r
#endif
int (*foo)(const char *, struct group *, char *, size_t , struct group **) = getgrnam_r;
int main(void) { return 0; }
