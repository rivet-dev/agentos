#include <unistd.h>
#ifdef fpathconf
#undef fpathconf
#endif
long (*foo)(int, int) = fpathconf;
int main(void) { return 0; }
