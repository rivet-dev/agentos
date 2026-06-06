#include <stdatomic.h>
#ifdef atomic_flag_clear
#undef atomic_flag_clear
#endif
void (*foo)(volatile atomic_flag *) = atomic_flag_clear;
int main(void) { return 0; }
