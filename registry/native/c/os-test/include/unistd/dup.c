#include <unistd.h>
#ifdef dup
#undef dup
#endif
int (*foo)(int) = dup;
int main(void) { return 0; }
