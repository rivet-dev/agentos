#include <grp.h>
#ifdef getgrnam
#undef getgrnam
#endif
struct group *(*foo)(const char *) = getgrnam;
int main(void) { return 0; }
