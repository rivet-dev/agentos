#include <stdatomic.h>
#ifdef atomic_flag_clear_explicit
#undef atomic_flag_clear_explicit
#endif
void (*foo)(volatile atomic_flag *, memory_order) = atomic_flag_clear_explicit;
int main(void) { return 0; }
