#include <unistd.h>
#ifdef symlinkat
#undef symlinkat
#endif
int (*foo)(const char *, int, const char *) = symlinkat;
int main(void) { return 0; }
