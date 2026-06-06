#include <unistd.h>
#ifdef lchown
#undef lchown
#endif
int (*foo)(const char *, uid_t, gid_t) = lchown;
int main(void) { return 0; }
