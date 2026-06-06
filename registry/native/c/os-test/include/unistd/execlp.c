#include <unistd.h>
#ifdef execlp
#undef execlp
#endif
int (*foo)(const char *, const char *, ...) = execlp;
int main(void) { return 0; }
