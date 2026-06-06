#include <pwd.h>
#ifdef getpwuid
#undef getpwuid
#endif
struct passwd *(*foo)(uid_t) = getpwuid;
int main(void) { return 0; }
