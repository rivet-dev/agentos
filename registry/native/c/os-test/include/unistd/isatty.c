#include <unistd.h>
#ifdef isatty
#undef isatty
#endif
int (*foo)(int) = isatty;
int main(void) { return 0; }
