#include <pwd.h>
#ifdef getpwnam
#undef getpwnam
#endif
struct passwd *(*foo)(const char *) = getpwnam;
int main(void) { return 0; }
