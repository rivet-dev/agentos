#include <grp.h>
#ifdef getgrgid_r
#undef getgrgid_r
#endif
int (*foo)(gid_t, struct group *, char *, size_t, struct group **) = getgrgid_r;
int main(void) { return 0; }
