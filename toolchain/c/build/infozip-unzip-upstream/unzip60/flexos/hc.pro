pragma Off(Floating_point);
pragma On(286);
pragma On(Literals_in_code);
pragma On(Warn);
pragma On(pointers_compatible);
pragma On(Callee_pops_when_possible);

pragma On(Auto_reg_alloc);
pragma On(Const_in_Code);
pragma On(Read_only_strings);
pragma On(Optimize_for_space);

pragma Off(Prototype_override_warnings);
pragma Off(Quiet);
pragma Off(Asm);
pragma Off(flexview);

#define PORTLIB
#define FLEXOS 1
