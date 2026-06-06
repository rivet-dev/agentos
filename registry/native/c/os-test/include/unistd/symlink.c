#include <unistd.h>
#ifdef symlink
#undef symlink
#endif
int (*foo)(const char *, const char *) = symlink;
int main(void) { return 0; }
