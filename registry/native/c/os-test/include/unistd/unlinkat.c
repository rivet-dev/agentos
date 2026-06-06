#include <unistd.h>
#ifdef unlinkat
#undef unlinkat
#endif
int (*foo)(int, const char *, int) = unlinkat;
int main(void) { return 0; }
