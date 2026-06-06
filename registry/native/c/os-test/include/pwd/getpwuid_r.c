#include <pwd.h>
#ifdef getpwuid_r
#undef getpwuid_r
#endif
int (*foo)(uid_t, struct passwd *, char *, size_t, struct passwd **) = getpwuid_r;
int main(void) { return 0; }
