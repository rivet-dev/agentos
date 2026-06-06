#include <pwd.h>
#ifdef getpwnam_r
#undef getpwnam_r
#endif
int (*foo)(const char *, struct passwd *, char *, size_t, struct passwd **) = getpwnam_r;
int main(void) { return 0; }
