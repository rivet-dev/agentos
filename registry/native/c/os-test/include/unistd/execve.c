#include <unistd.h>
#ifdef execve
#undef execve
#endif
int (*foo)(const char *, char *const [], char *const []) = execve;
int main(void) { return 0; }
