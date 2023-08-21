use crate::{
    ast::{Ast, AstKind},
    bytecode::RegisterID,
    symbol_table::{SymbolTable, Variable},
    types::{DataType, IntType},
    CompilerResult,
};
use std::{borrow::Cow, cmp::Eq, error::Error, fmt, iter::Peekable, str::CharIndices};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind<'src> {
    Ident,
    Number(u64),
    Str(Cow<'src, str>),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equals,
    Not,
    NotEquals,
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    LParen,
    RParen,
    LCurly,
    RCurly,
    LSquare,
    RSquare,
    Hash,
    AtSymbol,
    SemiColon,
    Colon,
    Comma,
    Assign,
    Function,
    Let,
    If,
    Else,
    While,
    True,
    False,
}

impl<'src> TokenKind<'src> {
    fn prefix_bp(&self) -> Option<usize> {
        match self {
            Self::Not | Self::Hash | Self::AtSymbol | Self::Sub => Some(7),
            _ => None,
        }
    }

    fn infix_bp(&self) -> Option<(usize, usize)> {
        match self {
            Self::Equals
            | Self::NotEquals
            | Self::Greater
            | Self::Less
            | Self::GreaterOrEqual
            | Self::LessOrEqual => Some((1, 2)),
            Self::Add | Self::Sub => Some((3, 4)),
            Self::Mul | Self::Div | Self::Mod => Some((5, 6)),
            _ => None,
        }
    }

    fn is_node(&self) -> bool {
        matches!(
            self,
            Self::Ident | Self::Number(_) | Self::Str(_) | Self::True | Self::False
        )
    }
}

#[derive(PartialEq, Eq, Clone)]
pub struct Token<'src> {
    pub text: &'src str,
    pub kind: TokenKind<'src>,
}

impl<'src> fmt::Debug for Token<'src> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            TokenKind::Ident => write!(f, "{}", self.text),
            TokenKind::Number(n) => write!(f, "{n}"),
            TokenKind::Str(ref string) => write!(f, "{string:?}"),
            TokenKind::Add => write!(f, "+"),
            TokenKind::Sub => write!(f, "-"),
            TokenKind::Mul => write!(f, "*"),
            TokenKind::Div => write!(f, "/"),
            TokenKind::Mod => write!(f, "%"),
            TokenKind::Equals => write!(f, "=="),
            TokenKind::Not => write!(f, "!"),
            TokenKind::NotEquals => write!(f, "!="),
            TokenKind::Greater => write!(f, ">"),
            TokenKind::Less => write!(f, "<"),
            TokenKind::GreaterOrEqual => write!(f, ">="),
            TokenKind::LessOrEqual => write!(f, "<="),
            TokenKind::LParen => write!(f, "("),
            TokenKind::RParen => write!(f, ")"),
            TokenKind::LCurly => write!(f, "{{"),
            TokenKind::RCurly => write!(f, "}}"),
            TokenKind::LSquare => write!(f, "["),
            TokenKind::RSquare => write!(f, "]"),
            TokenKind::Hash => write!(f, "#"),
            TokenKind::AtSymbol => write!(f, "@"),
            TokenKind::SemiColon => write!(f, ";"),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Assign => write!(f, "="),
            TokenKind::Function => write!(f, "fn"),
            TokenKind::Let => write!(f, "let"),
            TokenKind::If => write!(f, "if"),
            TokenKind::Else => write!(f, "else"),
            TokenKind::While => write!(f, "while"),
            TokenKind::True => write!(f, "true"),
            TokenKind::False => write!(f, "false"),
        }
    }
}

#[derive(Clone)]
pub enum ParseError<'src> {
    InvalidChar(char),
    UnclosedString,
    UnclosedParen(Token<'src>),
    UnexpectedToken(Option<Token<'src>>),
}

impl<'src> Error for ParseError<'src> {}

impl<'src> fmt::Debug for ParseError<'src> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidChar(ch) => write!(f, "Unexpected character `{ch}`."),
            Self::UnclosedString => write!(f, "Unclosed string."),
            Self::UnclosedParen(_) => write!(f, "Unclosed parentheses."),
            Self::UnexpectedToken(Some(token)) => write!(f, "Unexpected token `{token:?}`"),
            Self::UnexpectedToken(None) => write!(f, "Unexpected EOF."),
        }
    }
}

impl<'src> fmt::Display for ParseError<'src> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

fn parse_string(string: &'_ str) -> Cow<'_, str> {
    let mut value = Cow::Borrowed(string);

    let mut iter = string.char_indices();

    while let Some((pos, ch)) = iter.next() {
        if ch != '\\' {
            if let Cow::Owned(ref mut string) = value {
                string.push(ch);
            }
            continue;
        }

        let mut string = value.into_owned();
        string.truncate(pos);

        let Some((_, ch)) = iter.next() else {
            unreachable!("String should've been checked beforehand.");
        };

        string.push(match ch {
            'r' => '\r',
            'n' => '\n',
            't' => '\t',
            '0' => '\0',
            other => other,
        });

        value = Cow::Owned(string);
    }

    value
}

#[derive(Clone)]
pub struct Parser<'src> {
    string: &'src str,
    chars: Peekable<CharIndices<'src>>,
}

impl<'src> Parser<'src> {
    pub fn parse(string: &'src str) -> CompilerResult<'src, (Ast<'src>, SymbolTable<'src>)> {
        let mut symbol_table = SymbolTable::new();

        let mut parser = Self {
            string,
            chars: string.char_indices().peekable(),
        };

        let mut ast = parser.parse_expr_bp(&mut symbol_table, 0)?;

        if let Some(token) = parser.next_token()? {
            return Err(ParseError::UnexpectedToken(Some(token)).into());
        }

        DataType::Int(IntType::U64).infer(&mut ast)?;

        Ok((ast, symbol_table))
    }

    #[inline]
    fn peeking_char(&mut self, function: impl Fn(char) -> bool) -> bool {
        self.chars.peek().map_or(false, |&(_, ch)| function(ch))
    }

    #[inline]
    fn advance(&mut self, pos: &mut usize) {
        if let Some((new_pos, _)) = self.chars.next() {
            *pos = new_pos;
        }
    }

    fn next_token(&mut self) -> CompilerResult<'src, Option<Token<'src>>> {
        while let Some(&(mut pos @ start_pos, ch)) = self.chars.peek() {
            if ch.is_ascii_whitespace() {
                while self.peeking_char(|ch| ch.is_ascii_whitespace()) {
                    self.advance(&mut pos);
                }

                continue;
            }

            if ch.is_ascii_alphabetic() || ch == '_' {
                while self.peeking_char(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                    self.advance(&mut pos);
                }

                let text = &self.string[start_pos..=pos];

                return Ok(Some(Token {
                    text,
                    kind: match text {
                        "fn" => TokenKind::Function,
                        "let" => TokenKind::Let,
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "while" => TokenKind::While,
                        "true" => TokenKind::True,
                        "false" => TokenKind::False,
                        _ => TokenKind::Ident,
                    },
                }));
            }

            if ch.is_ascii_digit() {
                while self.peeking_char(|ch| ch.is_ascii_digit()) {
                    self.advance(&mut pos);
                }

                let text = &self.string[start_pos..=pos];

                return Ok(Some(Token {
                    text,
                    kind: TokenKind::Number(text.parse()?),
                }));
            }

            if ch == '"' {
                self.advance(&mut pos);

                while !self.peeking_char(|ch| ch == '"') {
                    if self.peeking_char(|ch| ch == '\\') {
                        self.advance(&mut pos);
                    }

                    self.advance(&mut pos);

                    if self.chars.peek().is_none() {
                        return Err(ParseError::UnclosedString.into());
                    }
                }

                self.advance(&mut pos);

                let text = &self.string[start_pos..=pos];

                return Ok(Some(Token {
                    text,
                    kind: TokenKind::Str(parse_string(&text[1..text.len() - 1])),
                }));
            }

            self.advance(&mut pos);

            let text = &self.string[start_pos..=pos];

            return Ok(Some(Token {
                text,
                kind: match ch {
                    '+' => TokenKind::Add,
                    '-' => TokenKind::Sub,
                    '*' => TokenKind::Mul,
                    '/' => TokenKind::Div,
                    '%' => TokenKind::Mod,
                    '=' => {
                        if self.peeking_char(|ch| ch == '=') {
                            self.advance(&mut pos);
                            TokenKind::Equals
                        } else {
                            TokenKind::Assign
                        }
                    }
                    '!' => {
                        if self.peeking_char(|ch| ch == '=') {
                            self.advance(&mut pos);
                            TokenKind::NotEquals
                        } else {
                            TokenKind::Not
                        }
                    }
                    '>' => {
                        if self.peeking_char(|ch| ch == '=') {
                            self.advance(&mut pos);
                            TokenKind::GreaterOrEqual
                        } else {
                            TokenKind::Greater
                        }
                    }
                    '<' => {
                        if self.peeking_char(|ch| ch == '=') {
                            self.advance(&mut pos);
                            TokenKind::LessOrEqual
                        } else {
                            TokenKind::Less
                        }
                    }
                    '(' => TokenKind::LParen,
                    ')' => TokenKind::RParen,
                    '{' => TokenKind::LCurly,
                    '}' => TokenKind::RCurly,
                    '[' => TokenKind::LSquare,
                    ']' => TokenKind::RSquare,
                    '#' => TokenKind::Hash,
                    '@' => TokenKind::AtSymbol,
                    ';' => TokenKind::SemiColon,
                    ':' => TokenKind::Colon,
                    ',' => TokenKind::Comma,
                    other => return Err(ParseError::InvalidChar(other).into()),
                },
            }));
        }

        Ok(None)
    }

    #[inline(always)]
    fn peek_token(&mut self) -> CompilerResult<'src, Option<Token<'src>>> {
        let prev_self = self.clone(); // Fast, shallow copy
        let token = self.next_token();
        *self = prev_self;
        token
    }

    #[inline(always)]
    fn expect_token(&mut self, kind: TokenKind<'src>) -> CompilerResult<'src, Token<'src>> {
        let token = self
            .next_token()?
            .ok_or(ParseError::UnexpectedToken(None))?;

        if token.kind == kind {
            Ok(token)
        } else {
            Err(ParseError::UnexpectedToken(Some(token)).into())
        }
    }

    #[inline(always)]
    fn peeking_token(&mut self, kind: TokenKind<'src>) -> CompilerResult<'src, bool> {
        Ok(self.peek_token()?.map_or(false, |token| token.kind == kind))
    }

    fn parse_let_statement(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
        argument_id: Option<RegisterID>,
    ) -> CompilerResult<'src, Ast<'src>> {
        self.expect_token(TokenKind::Let)?;

        let name = self.expect_token(TokenKind::Ident)?;

        self.expect_token(TokenKind::Colon)?;

        let data_type = self.parse_data_type()?;

        let value = if self.peeking_token(TokenKind::Assign)? {
            self.next_token()?;
            Some(Box::new(self.parse_expr_bp(symbol_table, 0)?))
        } else {
            None
        };

        symbol_table.add_variable(
            name.text,
            Variable {
                data_type: data_type.clone(),
            },
        );

        Ast::new(
            symbol_table,
            AstKind::Declaration {
                name: name.text,
                data_type,
                argument_id,
                value,
            },
        )
    }

    fn parse_function_definition(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
    ) -> CompilerResult<'src, Ast<'src>> {
        self.expect_token(TokenKind::Function)?;

        let name = self.expect_token(TokenKind::Ident)?;

        self.expect_token(TokenKind::LParen)?;

        let outer_scope_id = symbol_table.get_scope();

        let scope_id = symbol_table.add_scope();

        let mut arguments = Vec::new();

        if !self.peeking_token(TokenKind::RParen)? {
            loop {
                arguments.push(self.parse_let_statement(symbol_table, Some(arguments.len()))?);

                if !self.peeking_token(TokenKind::Comma)? {
                    break;
                }

                self.next_token()?;
            }
        }

        let argument_types = arguments
            .iter()
            .map(|argument| {
                let AstKind::Declaration { ref data_type, .. } = &argument.kind else {
                    unreachable!("There should only be declarations in function argument position")
                };

                data_type.clone()
            })
            .collect();

        self.expect_token(TokenKind::RParen)?;

        let return_type = if self.peeking_token(TokenKind::Colon)? {
            self.next_token()?;

            self.parse_data_type()?
        } else {
            DataType::Void
        };

        symbol_table.enter_scope(outer_scope_id);

        symbol_table.add_variable(
            name.text,
            Variable {
                data_type: DataType::Function {
                    return_type: Box::new(return_type.clone()),
                    argument_types,
                },
            },
        );

        symbol_table.enter_scope(scope_id);

        let body = self.parse_block(symbol_table)?;

        symbol_table.leave_scope();

        Ast::new(
            symbol_table,
            AstKind::FunctionDeclaration {
                name: name.text,
                scope_id,
                return_type,
                arguments,
                body: Box::new(body),
            },
        )
    }

    fn parse_statement(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
    ) -> CompilerResult<'src, Ast<'src>> {
        if self.peeking_token(TokenKind::Let)? {
            return self.parse_let_statement(symbol_table, None);
        }

        if self.peeking_token(TokenKind::Function)? {
            return self.parse_function_definition(symbol_table);
        }

        let lhs = self.parse_expr_bp(symbol_table, 0)?;

        if !self.peeking_token(TokenKind::Assign)? {
            return Ok(lhs);
        }

        self.next_token()?;

        let rhs = self.parse_expr_bp(symbol_table, 0)?;

        Ast::new(
            symbol_table,
            AstKind::Assign {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        )
    }

    fn parse_block(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
    ) -> CompilerResult<'src, Ast<'src>> {
        let scope_id = symbol_table.add_scope();

        let mut statements: Vec<Ast<'src>> = Vec::new();

        self.next_token()?;

        while !self.peeking_token(TokenKind::RCurly)? {
            let statement = self.parse_statement(symbol_table)?;

            statements.push(statement);

            self.expect_token(TokenKind::SemiColon)?;
        }

        self.next_token()?;

        symbol_table.leave_scope();

        Ast::new(
            symbol_table,
            AstKind::Block {
                statements,
                scope_id,
            },
        )
    }

    fn parse_data_type(&mut self) -> CompilerResult<'src, DataType> {
        let Some(token) = self.next_token()? else {
            return Err(ParseError::UnexpectedToken(None).into());
        };

        let data_type = match token.kind {
            TokenKind::Ident => match token.text {
                "Void" => DataType::Void,
                "Bool" => DataType::Bool,
                "S8" => DataType::Int(IntType::S8),
                "S16" => DataType::Int(IntType::S16),
                "S32" => DataType::Int(IntType::S32),
                "S64" => DataType::Int(IntType::S64),
                "U8" => DataType::Int(IntType::U8),
                "U16" => DataType::Int(IntType::U16),
                "U32" => DataType::Int(IntType::U32),
                "U64" => DataType::Int(IntType::U64),
                "String" => DataType::Ref(Box::new(DataType::Int(IntType::U8))),
                _ => return Err(ParseError::UnexpectedToken(Some(token)).into()),
            },
            TokenKind::Hash => DataType::Ref(Box::new(self.parse_data_type()?)),
            _ => return Err(ParseError::UnexpectedToken(Some(token)).into()),
        };

        Ok(data_type)
    }

    fn parse_if_statement(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
    ) -> CompilerResult<'src, Ast<'src>> {
        self.expect_token(TokenKind::If)?;

        let condition = self.parse_expr_bp(symbol_table, 0)?;

        let if_block = self.parse_block(symbol_table)?;

        let else_block = if self.peeking_token(TokenKind::Else)? {
            self.next_token()?;

            Some(Box::new(self.parse_block(symbol_table)?))
        } else {
            None
        };

        Ast::new(
            symbol_table,
            AstKind::IfStatement {
                condition: Box::new(condition),
                if_block: Box::new(if_block),
                else_block,
            },
        )
    }

    fn parse_while_loop(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
    ) -> CompilerResult<'src, Ast<'src>> {
        self.expect_token(TokenKind::While)?;

        let condition = self.parse_expr_bp(symbol_table, 0)?;

        let body = self.parse_block(symbol_table)?;

        Ast::new(
            symbol_table,
            AstKind::WhileLoop {
                condition: Box::new(condition),
                body: Box::new(body),
            },
        )
    }

    fn parse_function_call_args(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
    ) -> CompilerResult<'src, Vec<Ast<'src>>> {
        self.expect_token(TokenKind::LParen)?;

        let mut arguments = Vec::new();

        if self.peeking_token(TokenKind::RParen)? {
            self.next_token()?;
            return Ok(arguments);
        }

        loop {
            arguments.push(self.parse_expr_bp(symbol_table, 0)?);

            if !self.peeking_token(TokenKind::Comma)? {
                self.expect_token(TokenKind::RParen)?;
                break;
            }

            self.next_token()?;
        }

        Ok(arguments)
    }

    fn parse_expr_bp(
        &mut self,
        symbol_table: &mut SymbolTable<'src>,
        min_bp: usize,
    ) -> CompilerResult<'src, Ast<'src>> {
        let token = self
            .peek_token()?
            .ok_or(ParseError::UnexpectedToken(None))?;

        let mut lhs = match &token.kind {
            TokenKind::LParen => {
                self.next_token()?;

                let inside = self.parse_expr_bp(symbol_table, 0)?;

                self.expect_token(TokenKind::RParen)?;

                inside
            }
            TokenKind::LCurly => self.parse_block(symbol_table)?,
            TokenKind::If => self.parse_if_statement(symbol_table)?,
            TokenKind::While => self.parse_while_loop(symbol_table)?,
            kind if kind.is_node() => {
                self.next_token()?;

                Ast::new(symbol_table, AstKind::Node { token })?
            }
            other => {
                self.next_token()?;

                let Some(prefix_bp) = other.prefix_bp() else {
                    return Err(ParseError::UnexpectedToken(Some(token)).into());
                };

                let node = self.parse_expr_bp(symbol_table, prefix_bp)?;

                Ast::new(
                    symbol_table,
                    AstKind::Prefix {
                        oper: token,
                        node: Box::new(node),
                    },
                )?
            }
        };

        while let Some(oper) = self.peek_token()? {
            if oper.kind == TokenKind::LParen {
                let arguments = self.parse_function_call_args(symbol_table)?;

                lhs = Ast::new(
                    symbol_table,
                    AstKind::Call {
                        lhs: Box::new(lhs),
                        arguments,
                    },
                )?;
                continue;
            }

            let Some((infix_left_bp, infix_right_bp)) = oper.kind.infix_bp() else {
                break;
            };

            if infix_left_bp < min_bp {
                break;
            };

            self.next_token()?;

            let rhs = self.parse_expr_bp(symbol_table, infix_right_bp)?;

            lhs = Ast::new(
                symbol_table,
                AstKind::Infix {
                    oper,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            )?;
        }

        Ok(lhs)
    }
}
