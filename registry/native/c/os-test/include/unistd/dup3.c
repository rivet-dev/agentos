#include <unistd.h>
#ifdef dup3
#undef dup3
#endif
int (*foo)(int, int, int) = dup3;
int main(void) { return 0; }
